---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: 5c496cf2-d923-4cdb-bff1-73ad7c75c246
  codex: 01a17616-9ddc-7ee0-86df-a42c74605356
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
presets:
  '#agent-doc-causes': 'Fix agent-doc causes'
  '#audit': 'Audit agent-doc runtime for dogfooding. Add issues to the backlog + queue.'
  '#fix-dataflow': 'Fix any issue you see at multiple levels of the dataflow.'
  '#repo': 'https://github.com/btakita/agent-doc'
  '#self': 'agent-doc-bugs.md'
dogfood:
  path: tasks/agent-doc/agent-doc-bugs.md
  auto: true
queue: go
---

## Status <!-- agent:status patch=replace sync="##sync-release" -->

Main is at `b6b9af6e15` and agent-doc 0.35.532 is installed locally. Release v0.35.532 was published from `71f9801b49`. Post-release main now includes the inline-code-safe, occurrence-ordered queue preset authorization fix for GitHub issue #331 and the positive-evidence-only Remote Dev pane-order fix for GitHub issue #332; both issues are closed, the integrated full suite passed, and the current mainline was installed locally.

<!-- status:sync -->
Latest release: agent-doc v0.35.558 (release commit `00caa013ac`; GitHub Latest with Linux gnu/musl/aarch64, macOS and Windows archives, JetBrains 0.2.865/0.2.867/0.2.866 and VS Code 0.2.89 assets; PyPI 0.35.558; Homebrew bumped). Main is now at `a4a550c3a4`, unreleased and installed locally: struck-parent children close and a new child reopens (`ddc48d30e3`), reported rows close via nested echo and `queue consume --ack-id` (`dd65b2dfa9`), PM-sync design decisions (docs, `7f1bf60b7e`), Todo-backed queue rows kept in sync with their Todo (`a4a550c3a4`).
<!-- /status:sync -->

Queue markers: 📥 pending · 🟡 claimed · 🚧 running · ☑️ reported · ⚠️ lost · 🔄 needs reconcile · ❔ unknown · ⏭️ skipped · ⏳ awaiting operator · 🙋 operator action · 📌 pinned · 📍 agent pin ([legend](https://github.com/btakita/agent-doc/blob/main/docs/guide/queue-markers.md#queue-markers))
<!-- /agent:status -->
 

## Exchange <!-- agent:exchange patch=append subagents -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20261027-124849.md`*

Compacted content:
- Archived 38 response topic(s): session resumed after /clear; do [#decisionbacklogstrike]; id-less run + loop, no-arg entrypoint, stale-cell repair; 35 more
- Prior summary/context: prior compacted content: Archived 49 response topic(s): Subagent completed: `subagent:grok_queue_ui` returned work fo...

### Re: do [#clearqueuedciflake], do [#startcwdfallback], [#krwr] decision — opus-5 · 2026-10-27T08:58-04:00

Both are on `main` (`eebfdfef07..16484ef75b`, fast-forward) and installed locally. On the merge, `make check` passed 11897/11897 Rust (185 skipped) with 0 VS Code failures, and **`make tmux-ci` passed 206/206**. I ran tmux-ci too because this batch came from a tmux-ci failure.

| Item | Main commit | Fix |
|---|---|---|
| `#clearqueuedciflake` | `decfab70a3` | **This was a product race, not just a flaky test.** The supervisor's deferred-clear gate saw an idle pane, then read B's dispatch receipt as two separate queries ("running?", then "pending?"). If B started between those reads, both said no and `/clear` was typed into B's new turn. The `/clear` key handler had the same two-read gap. Both now use one `open_dispatch_turn_as_of` query. Load alone couldn't reproduce it (0/25 under stress), so a debug-only hook now pauses at exactly that point. The new forced-interleaving test failed 3/3 on the old code with CI's exact assertion, and passes 5/5 with the fix. |
| `#startcwdfallback` | `16484ef75b` | `start` used the process cwd in five places. For a document with no project, its admission path created state, bound the actor, wrote the registry entry and started the controller in whatever project it was run from. For every document, it also read the `tmux_session` pin and the prior and existing registry entries from the cwd project, and wrote the dead-pin rebind into that project's `config.toml`. All three start paths now resolve the document's store once (`start_project_root`), and every consumer uses it. A new live tmux test covers it, and 4 mutations each fail it. |

**`[#krwr]` (your decision): dispatched.** A claimed row that is deleted, with a different row added at the same position, should now cancel the claim instead of handing it to the new row. The worker is implementing that, including telling a genuine in-place edit (which must still follow its claim) apart from a replacement, and changing which steering event the owning worker receives (`cancel_claimed` instead of `forward_to_owner`). I'll check off Todo `#krwr` when that work lands. Checking it off now would also strike the in-flight queue row.

**Filed:**
- Backlog (go queue): `#autoclearorphandrain`. During a supervisor re-exec, the controller's fallback drain skips the pending between-items `/clear`. It failed 1 of 3 loaded runs.
- Backlog (go queue): `#loadprojectfordoccwd`. Config loading still falls back to the cwd for a document with no project (19 caller files).
- Backlog (go queue): `#queuerowdup`. Each claimed row now appears twice in `agent:run`, once behind a `☑️` marker and once plain. Preflight then selected the plain copy as a drainable head; I didn't drain it inline.
- Icebox: `#rootlessstorecreate`. The first start of a document with no project now creates `.agent-doc/` in its parent directory.

### Re: [#ddj8] Is `~` as an agent-doc project safe? — opus-5 · 2026-10-27T09:09-04:00

Short answers: **tsift will not index your home directory.** Making `~` an agent-doc project **works but isn't a good idea.** And **an agent-doc session can't finish a cycle outside a git repo.** These come from a read-only research pass over agent-doc `16484ef75b` and tsift `5ebe6e3`, with experiments in a throwaway HOME. Nothing was changed.

**1. Would tsift index all of `~`? No.**
- tsift chooses its root from `.tsift/`, `.gitmodules` or `.git`. `.agent-doc/` is not one of them.
- It refuses to index `$HOME`, `$TMPDIR` or `/` unless `TSIFT_ALLOW_AMBIENT_ROOT=1` is set. That refusal was the earlier `~/.tsift` fix. In a temp HOME containing `.agent-doc/`, both `tsift status` and `tsift index` refused.
- agent-doc never runs `tsift init`, `index` or `status --fix` itself. Every tsift call it makes is skipped unless an index already exists.

**2. Is `~` as an agent-doc project safe? It works, but it costs a lot.**
- **One guard reads every markdown file under the project root.** It runs at preflight and session-check whenever a document deletes open ids. At `$HOME` that's about 277k directories walked and 20k `.md` files, 216 MB read. At agent-loop it's 12 MB.
- **Repos under `~` would split.** In a git repo without its own `.agent-doc/`, IPC routing would use the repo, while the state store, registry and controller sat at `$HOME`.
- **Editors ignore the root limit.** The JetBrains and VS Code plugins walk upward without one, so opening any loose `.md` under `~` starts a listener for `$HOME`.
- **`~/.agent-doc` already exists by accident.** agent-doc never meant it as user state; it appeared because commands ran with `~` as the working directory. It holds 1.8G now, of which 1.7G is a retired `live-buffer/` last written in July. It also has a controller record with a dead pid and registered scratch files, and it collects a debug log every day.

**3. Can a project work outside a git repo? Only partly.** With a `.agent-doc/` folder but no git, preflight runs. But `finalize` and `write --commit` refuse ("requires a git repository for session documents"), so the cycle never closes and its boundary marker stays in the document. With neither git nor `.agent-doc/`, preflight fails with "no project root found".

**Options:**
- **(a)** Never treat `$HOME`, `/` or the temp dir as a project root, as the tsift fix does, with an opt-in override. Move user-level logs to `$XDG_STATE_HOME`. Recorded as icebox `#homerootguard`.
- **(b)** Keep agent-doc activity in a child git repo such as `~/notes`, with its own `.agent-doc/`. That's the only notes setup where cycles complete.
- **(c)** Leave things as they are.

**I recommend (a)+(b).** I've added a Todo for your choice. You can archive or delete the existing `~/.agent-doc`; the 1.7G `live-buffer/` in it is retired data.

**Filed (concrete bugs found along the way, so they go to the backlog):**
- `#tracingrootceiling`: the debug-log location walks up from the current directory, ignoring the root ceiling. That's why `~/.agent-doc` gets a daily log, and it's also a test-isolation leak.
- `#commitoutsidegit`: `agent-doc commit` outside git falsely reports "already matches HEAD".
- `#livebuffergc`: retired `live-buffer/` stores are never cleaned up.

The four in-flight fixes (`#krwr`, `#autoclearorphandrain`, `#loadprojectfordoccwd`, `#queuerowdup`) are still running. Then I'll do the release you queued.

### Re: Subagent completed: `subagent:krwr` returned work for queue item “[#krwr]: deleting a claimed queue row while adding a different row at the same position should cancel the claim instead of handing it to the new row as an edit” — agent-doc
<!-- agent-doc-subagent-response sha256=4f7c4e132ea8922dc0c54b90f8e2fe7c1a5ca82ad2581c52a3a8c8f94be2f59f -->
> **Subagent response:**
>
> Branch fix/krwr-replace-cancels-claim, commit b147e288f8 on eebfdfef07 (pushed); env -u TMUX -u TMUX_PANE make check exit 0, 11897 passed (183 skipped).
> Root cause: pair_replaced_lines (agent-doc-queue/src/queue_head_alignment.rs) paired leftover rows of an equal-count replace hunk by position, so steering reported change=edited dispatch=forward_to_owner and the claim ledger moved the claim onto the unrelated new row.
> Edit-vs-replacement rule (in pair_queue_heads, shared by steering and the ledger): a new row is an edit of an old row only when (1) same work identity (same tracked id, any prose rewrite, anywhere in the queue), or (2) in one replace hunk, one text contains the other or their char similarity ratio >= 0.5 (the existing in-place-edit threshold, now EDIT_SIMILARITY_THRESHOLD); two different tracked ids never pair; a row gaining or losing its id pairs only by containment (prose-vs-id ratio is noise: 'release + publish' scored >0.5 against 'do [#ridernavp6republish]'). Positional pairing removed.
> Steering: no routing code changed; a replaced claimed row now arrives as a deletion, so append_claimed_queue_removals emits cancel_claimed with exact owner/worker/generation and records the attempt cancelled; the ledger leaves the claim on the deleted row where it closes; the new row is change=added with no owner. forward_to_owner only for rule-matching edits.
> Files: agent-doc-queue/src/{queue_head_alignment.rs,queue_claim.rs}, agent-doc-document-realtime/src/midturn_steering.rs, agent-doc-session-check-io/src/midturn_steering.rs, specs/07-orchestration-commands.md (#krwr; updated #queue-mutation-steering, #claimfollowstruck), specs/07-closeout-commands.md, SPEC.md, AGENTS.md, runbooks/respond.md + 5 installed copies.
> Tests: new a_same_slot_replacement_is_a_delete_plus_an_add, same_row_edits_still_pair, a_same_slot_replacement_cancels_the_owner_instead_of_forwarding (2 replacements + 2 edits), replacing_a_claimed_id_row_in_its_slot_cancels_but_a_same_id_rewrite_forwards; rewritten a_claim_follows_its_row_edit_but_not_a_replacement_or_distant_add and the tail of claimed_head_edit_wakes_the_idle_coordinator_and_forwards_to_owner. Mutations: positional fallback restored -> 5 tests red; containment dropped -> same_row_edits_still_pair red.
> Behavior change: an in-place tracked-id change (do [#a] -> do [#b], including an id typo fix) now cancels instead of forwarding.
<!-- agent-doc-subagent-completion key=4ca583d9060ecfb4588087e735cc3853ad7a4689edf4183c5cc54b3304eb09f0 -->

### Re: Subagent completed: `subagent:loadprojectfordoccwd` returned work for queue item “do [#loadprojectfordoccwd]” — agent-doc
<!-- agent-doc-subagent-response sha256=07bd8b40a8b15fd2f41780ab442ef827337d975e961cb0c30fef4f7f5e4c2562 -->
> **Subagent response:**
>
> Branch fix/loadprojectfordoccwd, commit ce7838a637 (pushed); make check (via digest-runner, env -u TMUX -u TMUX_PANE) exit 0, 11901 passed (185 skipped).
> Root cause: agent-doc-project-config-io/src/lib.rs:105-110 (pre-fix) load_project_for_doc fell back to load_project() (the cwd project's config) for a document with no .agent-doc ancestor, while start/claim/route/sync bind it in the canonical parent's store.
> Fix: project_store_for_doc calls the existing project_root_or_file_parent (single resolver; new dep on agent-doc-project-root-io, no cycle) and load_project_for_doc reads it via load_project_for_root; a not-yet-canonicalizable path keeps the old upward walk; a rootless store with no config reads defaults and reading never creates .agent-doc.
> Callers (~30 sites in 19 files): document callers fixed with no edit (commit-io, lint-io, preflight-io, idle_watch.rs:3671, supervisor-io, frontmatter-io session.rs:322, session-check-io guard_modes/steering_gate_log/midturn_steering, route-io authoritative_dispatch, session-accretion-io, src/ plan, orchestrate, terminal:253, session_actor_cmd, main.rs:5517); steering_cmd.rs:153 already passed a root; project_root_for_doc callers left (no project = defaults); src/describe_image.rs edited to keep its image-project-or-cwd vision config explicitly. Claim's cross-session check (cwd project_tmux_session()) and tmux_session_policy_for_scope now read claim's store_root via project_tmux_session_for_root / tmux_session_policy_for_root.
> Tests: unit load_project_for_doc_reads_the_document_store_never_the_cwd_project; 3 CLI tests on isolated -L tmux (claim from another project checks / enforces the document's tmux pin; rootless claim ignores the cwd pin). Mutations: restoring the load_project() fallback reds the unit test; claim reading the cwd pin reds all 3 CLI tests (multi-session-rules half untested). project-config-io 11 passed; test_cli claim_ 6 passed. Specs: specs/06-config.md, specs/07-session-tmux-commands.md.
> Follow-ups: (1) agent-doc-sync-io/src/resync.rs:267 resolve_registry_root falls back to current_dir() for a rootless target; recover_target_document_pane_in (resync.rs:380) and the registry repair (:623) read the cwd's project_tmux_session() with a document in hand. (2) src/terminal.rs reads the cwd registry (199) and cwd tmux_session pin (resolve_session_name, 306) for a document. (3) claim find_alive_project_window (--window fallback) reads the cwd registry. (4) tmux_session_policy_for_scope / sync_scope_root (agent-doc-sync/src/lib.rs:623) fall back to cwd for a rootless focused document; route session_resolution.rs:41/223 and sync resolve_preferred_session read the cwd pin without a document (likely process/layout level; decide).
<!-- agent-doc-subagent-completion key=59944363ba545f84f9cad856a96cec289dec2625d1ce8ff2c5327586ad388543 -->

### Re: Subagent completed: `subagent:queuerowdup` returned work for queue item “do [#queuerowdup]” — agent-doc
<!-- agent-doc-subagent-response sha256=e68334ee664390375746c6e3438723c99aab70cff5c7875f5a3a194c92072394 -->
> **Subagent response:**
>
> Branch fix/queuerowdup, commit dd3138e3c2 (pushed); env -u TMUX -u TMUX_PANE make check exit 0, 11900 passed (185 skipped). No production code changed: the premise was wrong.
> Finding: no binary writer added the duplicate rows; the operator typed them. ops.log: 12:47:41Z operator deleted the 504 bytes of struck rows; 12:47:57-12:48:01Z editor_ops_recorded with editor_op_capture_proof operator_ops=2 non_operator_ops=0 inserted 52 bytes at the queue end = the two plain rows (IDE list continuation, a 41-byte paste of "[#clearqueuedciflake]\n[#startcwdfallback]", a typed "- ", and "do " typed at two cursors). The 12:47:57Z queue_maintenance write (+14 bytes) started from the pre-typing text. The 12:48:49Z compact (controller_compact_document commit=true) committed the rows into the snapshot, after which preflight's dedup (converge_queue_via_lifecycle, agent-doc-queue/src/document_queue.rs) treated them as intended repeats; the 12:53:33Z queue_maintenance (+83 bytes) gave both copies the identity's marker (one marker target per row by design, agent-doc-preflight-io/src/lib.rs ~7193, queue_cmd.rs ~260). The selected head was queue:3:clearqueuedciflake:0, the marked Reported row (selectable for integration per #reportedwritebackdebt), not the plain twin.
> Writers checked: backlog mirror (enqueue_actionable_ids_reporting) is already marker-aware (queue_prompt_reference_ids reads every #id); the claim projection only rewrites the prefix; --done strikes both copies. Selection, drainable count and dispatch exclude by work identity, so a twin of Claimed/Running work is never drained inline or redispatched.
> Tests: backlog_sync mirror_never_re_adds_an_id_queued_behind_a_lifecycle_marker (7 marker shapes, prepend + append); CLI a_claimed_mirrored_id_head_stays_one_row_through_its_whole_attempt; CLI a_committed_plain_twin_of_a_running_claim_is_never_selected_or_dispatched (twin asserted present, never selected/dispatched, drainable 0). Each failed under a targeted mutation. Spec: specs/07-orchestration-commands.md #queuerowdup.
> Follow-ups (operator decisions): (1) the compact exchange commit absorbed an unanswered operator queue edit mid-turn (an earlier commit_document at 12:47:52Z had refused the same drift as an unanswered edit). (2) should a committed plain do [#id] twin of a claimed row be collapsed or flagged? Removing it would delete operator-typed text.
<!-- agent-doc-subagent-completion key=c32db00ee32284e36bac3265deeccbb3e8480f6f1c388bb775fe2cac70963c24 -->

### Re: Subagent completed: `subagent:tracingrootceiling` returned work for queue item “do [#tracingrootceiling]” — agent-doc
<!-- agent-doc-subagent-response sha256=4ad7513b884215f9cf845e319f80a2c86e862bf8f9a35369b1c72e74474eb4e8 -->
> **Subagent response:**
>
> Branch fix/tracingrootceiling, commit 21b4c27444 on 16484ef75b (pushed); env -u TMUX -u TMUX_PANE make check exit 0, 11900 passed (185 skipped).
> Root cause: src/main.rs:5260-5272 init_tracing walked up from cwd with no ceiling to the first ancestor that already had .agent-doc/logs/, ignoring AGENT_DOC_ROOT_CEILING_DIRECTORIES, and skipped a nearer project root without logs/ so a nested project's log landed in an outer one.
> Fix: tracing_log_dir(cwd) uses agent_doc_fs::find_project_root (ceiling-aware); the nearest project root owns <root>/.agent-doc/logs/ (created if missing, as ops.log does). No project root -> file logging off with a stderr note (the existing no-root behavior; no repo user-level log dir exists). Limit: with the ceiling unset, a cwd under $HOME outside any other project still logs to ~/.agent-doc/logs/ (SPEC.md:635 treats $HOME/.agent-doc as a fallback root; that is the #homerootguard decision).
> Other walkers: src/plugin.rs log_jetbrains_upgrade_decision_at and agent-doc-ops-log-io already go through project_root_containing (ceiling-aware); other ancestors()/pop() loops are test probes or non-.agent-doc walks. Out of scope: agent-doc-template-io load_project_from_doc (src/lib.rs:528-548) walks for .agent-doc/config.toml with no ceiling.
> Files: src/main.rs, tests/tracing_root_ceiling.rs (new), SPEC.md (#tracingrootceiling), specs/06-config.md (AGENT_DOC_LOG), docs/getting-started/configuration.md.
> Tests: tests/tracing_root_ceiling.rs (3): ceiling -> ancestor .agent-doc gets no files (no debug.log, no ops.log); control without ceiling shows the ancestor would get it; nearest root owns the log with no logs/ yet. 3 passed; restoring the unbounded walk reds the ceiling and nearest-root tests (control passes).
> Note: rustfmt on src/main.rs reformats every mod file it follows; reverted all but the hunk.
> Follow-up: route agent-doc-template-io load_project_from_doc (src/lib.rs:528-548) through ceiling-aware discovery.
<!-- agent-doc-subagent-completion key=98d497ecb3a471c5629b665cebf2d43b2006197c8e0613f6080d228146decbd9 -->

### Re: Subagent completed: `subagent:livebuffergc` returned work for queue item “do [#livebuffergc]” — agent-doc
<!-- agent-doc-subagent-response sha256=2bdc5fd3e539457147fd0dc9b861c97f569298294afdc314a389c16038ca1ac9 -->
> **Subagent response:**
>
> Branch fix/livebuffergc, commit f585039309 (pushed); env -u TMUX -u TMUX_PANE make check exit 0, 11906 passed (185 skipped). Nothing run against real stores (read-only inspection only).
> Retirement evidence: d52697f953 (2026-07-16, 0.34.160, "make Lazily the sole live editor authority"; changelog: live-buffer snapshots no longer written or read, legacy files delete-only) and ef00649dfa (2026-07-17, "make Lazily the sole live document authority"); on main no reader/writer, and test_editor_hot_path_has_no_filesystem_sidecar_or_reload_broadcast_transport already forbids .agent-doc/live-buffer in hot-path sources. ~/.agent-doc/live-buffer: 2,928 flat files, no subdirs, no symlinks.
> Interface: agent-doc gc --retired-stores --dry-run [--root DIR] lists each retired store (path, files, bytes, last write), deletes nothing; agent-doc gc --retired-stores [--root DIR] removes only <root>/.agent-doc/live-buffer. Home store: agent-doc gc --root ~ --retired-stores --dry-run, then without --dry-run (the flag skips the rest of gc, so no $HOME scan). Plain agent-doc gc (with or without --dry-run) only reports the store and prints those commands; preflight auto-gc never reports or removes it; --retired-stores conflicts with --database-only. Rationale: gc deletes by default and preflight auto-gc + the JetBrains "GC Stale Sessions" menu run plain gc, so removal needs an explicit operator flag.
> Safety: symlinks never followed (a link inside is unlinked, target untouched); a symlinked or non-directory store root is refused; partial failures continue, are printed, exit non-zero.
> Files: agent-doc-gc-io/src/retired_store.rs (new, 7 unit tests), agent-doc-gc-io/src/lib.rs, src/main.rs, tests/test_cli.rs (e2e + source guard), specs/07-core-commands.md.
> Tests: gc-io 23 passed; test_cli 4 passed (test_gc_retired_stores_lists_then_removes_only_the_live_buffer_store, test_retired_store_removal_is_reachable_only_from_explicit_gc_flag, existing hot-path guard, --database-only). 6 mutants all killed (symlinked-root test tightened to require the symlink reason).
> Follow-ups: other apparently retired dirs under real .agent-doc stores (patches/, plugin-owner/, ack-content/, op-capture/; also forbidden by the hot-path guard) could join the retired-store list after the same retirement check.
<!-- agent-doc-subagent-completion key=8899eab8e5d2e2668319b6a533c4049cf8a9e83e7ed661b2f2d80e263152bf30 -->

### Re: Subagent completed: `subagent:commitoutsidegit` returned work for queue item “do [#commitoutsidegit]” — agent-doc
<!-- agent-doc-subagent-response sha256=7afd381f7cb9914bce2651f054c599d6d2577a2ca74ff893204f47cd1bd2b82f -->
> **Subagent response:**
>
> Branch fix/commitoutsidegit, commit 7c517bdfdc on 16484ef75b (pushed); second env -u TMUX -u TMUX_PANE make check exit 0, 11902 passed (185 skipped).
> Root cause: agent-doc-git-io/src/dirs.rs:151 resolve_absolute_to_git_root falls back to the process cwd as the "git root" outside git, so every git step no-ops, commit_with_ports_outcome returns did_commit=false and src/main.rs (~5918, commit_outcome_message 183-190) prints "nothing to commit: ... already matches HEAD" with exit 0, after already snapshotting and moving the boundary marker.
> Fix: agent_doc_git_io::status::require_git_repository_for_commit (resolves a relative FILE against cwd; actionable error naming git init / moving the document; a missing file falls through to the command's own not-found error); agent-doc commit calls it before staging or touching the file; compact --commit (agent-doc-compact-io, controller entry point so editor compacts too) refuses after the integrity/lint gates and before any capture/tag/archive/commit (it previously compacted then blamed a stale-supervisor CRDT replay). write --commit / finalize already refuse; non-session write --commit still skips silently per its help; NotInGitRepo session-check guards never claim a commit. Spec: specs/07-closeout-commands.md #commitoutsidegit.
> Tests: tests/commit_outside_git.rs (5): commit outside git with/without a .agent-doc ancestor, relative path, compact --commit (document bytes unchanged), relative path into a repo from a non-git cwd still commits; all assert exit code, message, file unmodified; fixtures set AGENT_DOC_ROOT_CEILING_DIRECTORIES. Mutations M1 (always pass) -> 4 refusal tests red; M2 (no relative resolution) -> in-repo test red. compact-io lib 79 passed. First make check failed on two existing source-order guards (compact_commit_leg_has_no_silent_skip, compact_integrity_gate_fails_before_tag_archive_or_document_mutation), resolved by moving the check after the integrity gate.
> Notes: an early repro inside the worktree made two local commits, removed with git reset --hard 16484ef75b before work began (never pushed); 3 repro controllers stopped by exact pid.
> Follow-ups: none (optional: notify quietly no-ops outside git without claiming a commit).
<!-- agent-doc-subagent-completion key=7fd4dbd77942461dcc8209fd11c78c6df08452fc11ac28ecf513958bddf35999 -->

### Re: Subagent completed: `subagent:autoclearorphandrain` returned work for queue item “do [#autoclearorphandrain]” — agent-doc
<!-- agent-doc-subagent-response sha256=135ce3aa33420c42c1f29e5adc0cdbf8d55e7dcc518ec84ead28b89142840542 -->
> **Subagent response:**
>
> Branch fix/autoclearorphandrain, commit 6be8a6340a (pushed); make check passed (11897); make tmux-ci passed (46 suites, 207 tests, 0 failed).
> Root cause: the orphan-drain sweep's liveness probe does not see a supervisor not started with --route-owned, so it treats the document as orphaned every tick; only route-time deferral to a healthy supervisor's idle watch (GH #307) prevents a double drive. Interleaving: item one commits; drain dispatches item two (reason=no_supervisor_idle_watch); supervisor re-execs (outgoing image publishes the recycle InFlight before exec; new image has not started IPC/registered); the route sees supervisor_health=unreachable (same as dead), takes route_dispatch_only_authoritative_degraded_direct_pane, records a dispatch attempt, waits for the recycle to settle, and types item two past the /clear the new image's idle watch carried across the re-exec. Faulty branch: background guard in agent-doc-route-io/src/pane_resolution.rs (~540 pre-change) deferred only for a healthy actor.
> Fix: three-case supervisor policy (pane_resolution.rs:125, check :540): Recovered (healthy) -> defer then fallback (existing); Restarting (not healthy, recycle InFlight) -> defer (outcome=deferred reason=supervisor_restarting), next attempt for the same head continues as orphan (restarting_supervisor_missed_backoff action=continue_orphan_route); Absent -> unchanged orphan path. Deferral happens before any dispatch receipt. A real orphan still drains (no InFlight recycle or status read failure -> unchanged; a never-settling restart proceeds on the next attempt). Deferred rather than having the drain send the clear, keeping the idle watch as the sole owner of the clear decision.
> Tests: debug-only hooks AGENT_DOC_TEST_ORPHAN_DRAIN_GATE (rpc.rs:20397) and AGENT_DOC_TEST_HOLD_PRESERVED_CHILD_REENTRY (run.rs:38); new live test an_orphan_drain_during_a_preserved_child_reexec_defers_to_the_restarting_supervisor (before fix 3/3 failed with the CI assertion/ops.log pattern; after 5/5); whole file under stress before 12/13, after 13/13 x3; unit test for the policy; mutations (restarting check forced false -> live test red; Restarting as Absent -> unit test red).
> Files: agent-doc-route-io/src/pane_resolution.rs, agent-doc-controller-io/src/project_controller/rpc.rs, agent-doc-start-runtime-io/src/run.rs, tests/clear_queued_live_turn_tmux.rs, specs/supervisor.md, specs/08-session-routing.md, specs/07-session-tmux-commands.md.
> Follow-up: orphan-drain liveness probe misses operator-started supervisors (supervisor_pid_for_doc only matches 'start --route-owned'); decide whether to use start_supervisor_document_from_args (kill paths route-owned only) and its effect on the GH #307 fallback.
<!-- agent-doc-subagent-completion key=5b1b46162de66acfa5f0bc2abd1a40d573e9f7cc45d66782f4d4419d51203b95 -->
> **Queue prompt:** release + publish after "deleting a claimed queue row while adding a different row..." + [#autoclearorphandrain] [#loadprojectfordoccwd] [#queuerowdup] is integrated.

### Re: release v0.35.557 — opus-5 · 2026-10-27T12:02-04:00

**agent-doc v0.35.557 is released and published.** `make release` finished with `tag_publish_handoff=complete local_install_full=complete public_publication=complete`:
- tag `v0.35.557` on `fec25bdee1`
- GitHub **Latest** with 24 assets
- PyPI wheel `agent_doc-0.35.557` public
- `agent-doc 0.35.557` installed locally via `install-full`

The release batch was your four prerequisites (`#krwr`, `#autoclearorphandrain`, `#loadprojectfordoccwd`, `#queuerowdup`) plus `#tracingrootceiling`, `#commitoutsidegit` and `#livebuffergc`, which were ready at the same boundary. On the version-projected tree, `make check` passed 11923/11923 and `make tmux-ci` passed 207.

| Item | Commit | Fix |
|---|---|---|
| `[#krwr]` | `f1473ab067` | Deleting a claimed row while adding a different row in the same slot now cancels the claim (the owner gets `cancel_claimed`), and the new row stays unclaimed. The claim still follows a same-id rewrite or a close text edit (containment, or similarity ≥ 0.5). **Behaviour change:** an in-place id change, even a typo fix, now cancels. |
| `#autoclearorphandrain` | `02f41205a7` | While a supervisor is mid re-exec (recycle `InFlight`), the orphan-drain route now defers instead of typing the next item past the pending auto-clear. A real orphan still drains. The forced interleaving failed 3/3 before the fix and passes 5/5 after; the whole file under load passed 13/13 three times. |
| `#loadprojectfordoccwd` | `8c1dc136c6` | `load_project_for_doc` reads a rootless document's config from its own store, never the cwd project's. That fixes ~30 callers with no edits; claim's cross-session check and tmux-session rules now use claim's store. |
| `#queuerowdup` | `14bdb554fe` | **I was wrong:** no binary writer created those duplicate rows. ops.log shows you pasted and typed them at 12:47:57Z, and a `compact` committed them 52s later. The worker added regression tests confirming the mirror is marker-aware and that a twin of claimed work is never selected or dispatched. No production change. |
| `#tracingrootceiling` | `08dd87946c` | The debug log goes to the nearest project root, found by the ceiling-aware walk, or nowhere. It no longer lands in an ancestor such as `~/.agent-doc`. |
| `#commitoutsidegit` | `f7fd826e5d` | `agent-doc commit` outside git exits 1 with a `git init` hint instead of a false "already matches HEAD". `compact --commit` refuses before changing anything. |
| `#livebuffergc` | `953feb85a0` | `agent-doc gc --root ~ --retired-stores --dry-run` lists the retired 1.7G `~/.agent-doc/live-buffer`, and running it without `--dry-run` removes it. Plain `gc` and the daily preflight gc only report it. Nothing was deleted from your real stores. |

**Release-time fixes I made.**
- `make check` first failed the `editor-parity` gate, because each release needs one row in `editors/release-parity.tsv`. Adding that row exposed a gap: RTK had truncated `git log` to 50 of the 86 commits since `v0.35.556`, so my first `VERSIONS.md` draft missed every VS Code, Zed and native replica-flush change. I rebuilt the entry from the unfiltered list.
- `make release` then refused once, because I'd written my logs inside the worktree, which changed the check fingerprint. I moved the logs out and reran.

**Filed:**
- Backlog (go queue): `#cwdfallbackrest`. The remaining cwd fallbacks: resync, `terminal`, claim's `--window` fallback, and the template-io config walk.
- Backlog (go queue): `#compactabsorbsedit`. `compact` committed your unanswered queue edit mid-turn, although a `commit_document` 57s earlier refused the same drift.
- Icebox: `#orphandrainprobe`. The drain's liveness probe misses supervisors started without `--route-owned`.
- Icebox: `#retireddirsgc`. More retired `.agent-doc` dirs could join `--retired-stores`, and whether to collapse or flag a plain twin of a claimed row is still undecided.
- Waiting on you: Todo `#yeja`, how to handle `~/.agent-doc`.

### Re: do [#compactabsorbsedit], do [#cwdfallbackrest] — opus-5 · 2026-10-27T13:10-04:00

Both are on `main` (`fec25bdee1..c7e7de85fc`, fast-forward) and installed locally. They're not in a release yet; the next one after v0.35.557 will include them. On the merge, `make check` passed 11934/11934 and `make tmux-ci` passed 207.

| Item | Commit | Fix |
|---|---|---|
| `#compactabsorbsedit` | `63cbc266b0` | **You ran that compact**, from JetBrains "Compact Exchange" at 08:48:48 EDT. Compaction built its commit from the editor text, so your unanswered queue rows in other sections were committed with it. The unanswered-edit guard only fires when the snapshot equals HEAD, and compaction checkpoints first. A compact with commit now commits only the exchange it compacted. Your other edits stay live and uncommitted, and the next preflight still attributes them to you as `[user+]`. **Behaviour change:** scratch text outside the exchange, and a queue-row deletion made while the editor was paused, also stay out of the commit now. |
| `#cwdfallbackrest` | `c7e7de85fc` | The remaining document reads now resolve from the document's store: resync's registry root, preferred-window pin and WrongSession check, `terminal`'s registry and pin, claim's dead-`--window` fallback, and template-io's config walk, which now also honors the root ceiling. A targeted resync of a document with no project no longer touches the current directory's project. New tests check every byte of that project's `.agent-doc` is unchanged. |

**Filed to the icebox** (long-tail, not queued):
- `#cwdfallbacktail`: `resolve_pane_cwd` outside git, relative registry entries resolved against the cwd, and `terminal` ignoring `AGENT_DOC_TMUX_SOCKET`.
- `#compactrequester`: log who requested a compact, and decide whether operator edits inside the exchange should still be archived.

`#restartagentharness` (your Restart Agent → Grok Build report) is running in its own worktree.
> **Queue prompt:** I changed lazily.md from `agent: claude` to `agent: grok` then ran Restart Agent. The session restarted in Claude Code. It should have started Grok Build.

### Re: Subagent completed: `subagent:restartagentharness` returned work for queue item “I changed lazily.md from `agent: claude` to `agent: grok` then ran Restart Agent. The session restarted in Claude Code. It should have started Grok Build.” — agent-doc
<!-- agent-doc-subagent-response sha256=ada8e360a0d6a9d27fef7acfc67a5bd731e72a0999bafb529564417bfe2ac827 -->
> **Subagent response:**
>
> Branch fix/restartagentharness, commit fe32f5a781 (pushed; Kotlin changed, JetBrains plugin generation bump still needed, so make -k check fails only plugin-version-check; nextest 11929 passed). This does NOT make Restart Agent start Grok Build: it makes it refuse with both agent values named instead of silently relaunching Claude Code.
> Evidence: 16:31-16:34:53Z five restart_agent_menu_invoked, each agent_restart_respec_refreshed running_harness=claude resolved_harness=claude resolved_from=frontmatter:claude reading source=start_runtime_restart_frontmatter authority=cp_model text_hash=f22b45bd4a49 (128777 bytes = committed 206af5eef3, agent: claude). The operator's claude->grok edit was recorded (editor_ops_recorded at offset 168, lineage_post=8d23a9e29b5c = disk at the time) but never reached the controller: at 16:07:54Z a crdt_cp_write source=retained_transition_projection_effect (f22b45) landed while the operator typed; the editor's update was crdt_document_op_delta_ingested outcome=StructuralQuarantined -> crdt_replica_update_quarantined recovery=lazy_canonical_projection, never recovered (retained_persistence_visible_roll_forward_requested refusals=117 at 17:01Z) while logs reported delivery_converged=true diverged=false.
> Root cause: harness resolution is correct (agent-doc-supervisor/src/harness_authority.rs resolve_harness_authority puts the document's agent: first); agent-doc-start-runtime-io/src/run.rs:1690 try_resolve_current_document_content returns the stale controller copy because of the stuck sync (agent-doc-crdt-relay-io/src/lib.rs:3881-3900 ReprojectCanonical gate, 4096-4104 quarantine log). Also fixed: the resolved_from= log printed the post-override value.
> Change: agent-doc session restart-agent <FILE> --expect-agent <AGENT> compares the editor's agent with the controller copy's before any restart (claude/claude-code, grok/grok-build equivalent) and refuses on mismatch (restart_agent_refused reason=authority_lags_editor); no flag = unchanged. Not overriding with the editor value because the agent-change watch reads the same stale copy and would switch back. JetBrains: NativeLib.kt (agent_doc_frontmatter_agent export), TerminalUtil.kt (Restart Agent + --force retry pass --expect-agent), TerminalUtilTest.kt.
> Tests: supervisor restart_agent_agreement 2, ffi frontmatter_agent 1, bin restart_agent_ 4 (incl. source-order guard); mutations confirmed.
> Follow-ups: (1) URGENT lazily.md editor sync stuck since 16:07:54Z (StructuralQuarantined, refusals=117) with false delivery_converged=true / diverged=false; (2) VS Code Restart Agent needs the same --expect-agent; (3) track the id used in comments.
<!-- agent-doc-subagent-completion key=de31e99b6dd843240e5e9e4745bb3c6421ddb4d97d3b53fcc26243c5eb074444 -->
> **Queue prompt:** I changed lazily.md from `agent: claude` to `agent: grok` then ran Restart Agent. The session restarted in Claude Code. It should have started Grok Build.

### Re: Restart Agent kept Claude Code — and lazily.md lost your edits — opus-5 · 2026-10-27T13:17-04:00

**Your `tasks/software/lazily.md` edits were overwritten. Recover them from JetBrains Local History**, from the version before 13:09 EDT. That version has `agent: grok`, `queue: go` and your queue edits. The file on disk now holds a stale copy written at 13:10:19 EDT; it says `agent: claude`, at 128,777 bytes. No copy of your version survives in agent-doc's stores. I haven't touched the file, because the editor's text is the authority there and the IDE is the safe recovery path. I added a Todo for this.

**What happened** (agent-loop `.agent-doc/logs/ops.log`, times UTC):
1. **16:07:54.** A retained captured-response write (`crdt_cp_write source=retained_transition_projection_effect`, text `f22b45…`) landed while you were typing. The editor's update was quarantined (`StructuralQuarantined`) and never recovered: 117 refusals by 17:01. Meanwhile the logs claimed `delivery_converged=true diverged=false`.
2. **16:31–16:34.** Your five Restart Agent presses all read `agent: claude` from that stale controller copy, so Claude Code relaunched. Harness resolution itself is correct: the document's `agent:` wins. It was reading the wrong copy.
3. **17:09:03.** The editor still projected your version (`8d23a9…`).
4. **17:09:04.** The retained projection re-asserted `f22b45…`.
5. **About 17:09.** My `make install` reloaded the editor library. The new editor replicas re-registered from that stale text, and the stale copy reached disk at 17:10:19. So my install triggered the final overwrite, but the overwrite itself comes from the retained-projection bug.

**Filed and to be dispatched as urgent: `#retainedprojectionclobber`.** A retained captured projection must never overwrite text you wrote; your ops have to roll forward onto it. Quarantine recovery must either converge or fail loudly, and agent-doc must stop reporting "converged" while an editor is quarantined.

**The Restart Agent worker** (branch `fix/restartagentharness`, `fe32f5a781`, not merged yet) makes Restart Agent refuse and name both agent values when the editor and the controller disagree, instead of quietly relaunching the old harness. It doesn't make the switch work while the sync is stuck, so I'm holding it to merge with the clobber fix. It also changes Kotlin, so it needs a JetBrains plugin version bump. Your Restart Agent row stays open until the switch actually works.

### Re: lazily.md clobber fix, Restart Agent, JetBrains plugins, [#ahfe] — opus-5 · 2026-10-27T14:48-04:00

**The clobber fix and the Restart Agent check are on `main` and installed** (`c7e7de85fc..7a92742ece`, fast-forward). On the merge, `make check` passed 11946/11946 and `make tmux-ci` passed 207. Before installing I confirmed no editor had been quarantined in the previous 30 minutes. The JetBrains plugins are bumped to classic 0.2.865, 2026.2 0.2.867 and 2026.3 0.2.866, so **your 2026.3 IDE needs a restart.**

| Item | Commit | What changed |
|---|---|---|
| `#retainedprojectionclobber` | `fe3131b67a` | Four changes:<br>• A retained captured write can no longer replace your text. Frontmatter merges field by field, sections both sides changed merge 3-way, and a write that would drop a line you added is refused and logged (`retained_transition_refused_operator_text_loss`).<br>• A quarantined editor now fails loudly after 3 undelivered roll-forwards instead of retrying silently.<br>• An editor that is still quarantined holds the convergence barrier, so agent-doc stops reporting "converged".<br>• A reproduction test runs your exact sequence: retained write, quarantine, controller reseed, editor re-register. |
| Restart Agent (your Grok report) | `f111fc6288` | Restart Agent now compares the editor's `agent:` with the controller's copy and refuses, naming both values, instead of silently relaunching the old harness. Harness resolution itself was correct; it was reading the stale controller copy that the clobber bug left behind. |
| `#movedsingleplugin` (your question) | — | **No single JetBrains plugin.** One plugin ID ships three builds with disjoint IDE ranges: classic 2024.2–2026.1, exact 2026.2, exact 2026.3. 2026.2 and 2026.3 are the same code. Classic can't share a ZIP with the modular builds: they need platform modules that only exist from 2026.2, and Java 25. The 2026.2/2026.3 split is a policy clamp on internal APIs, not a code difference. **Recommendation:** keep classic, and decide whether to merge 2026.2+2026.3 into one modular ZIP (option B). |
| `[#ahfe]` (GH #267 live check) | — | **Not enough evidence.** Run Agent Doc worked after every hot reload today, but every one happened with a current plugin, so none tested the old-jar case. The 2026.3 detection is also blind (`#jb263probeblind`, filed). **Your IDE is in the test window right now:** restart is pending for 0.2.866. Before restarting, run Agent Doc once. It passes if `idea.log` shows `[route] SUCCESS`; expect no advisory line until `#jb263probeblind` lands. Then restart. GH #267 stays closed and `#ahfe` stays open. |

**`lazily.md`:** the controller is no longer stuck, and you had already re-typed `agent: grok`. Your other queue edits and `queue: go` from before 13:09 EDT can only be restored from JetBrains Local History (Todo `#v829`). After restarting the IDE, retry Restart Agent; that's a new Todo.

**Dispatched:**
- The parent-plus-subtasks worker (`feat/nestedsubprompts`): it implements nested sub-prompts as children of their parent across the list components, and writes a design doc for `agent:github`/`agent:gitlab`/`agent:jira` sync for your review. **Workaround:** the binary binds one worker to one row, so the other three subtask rows are bound to that same worker as `<worker>/sub2`, `/sub3` and `/sub4`. That's the limit this work removes.
- A design-only worker for your automation-API question (`design/automationapi`): should the controller be the API server?

**Filed:**
- Backlog (go queue): `#clobberfollowups` (why the replay built its target from stale text, refusing commits while an editor is quarantined, deriving `diverged`, surfacing the refusal in session-check).
- Backlog (go queue): `#jb263probeblind`.
- Icebox: `#jbdocdrift`.

### Re: Subagent completed: `subagent:automationapi` returned work for queue item “agent-doc should have an API for automation. This API should work across all languages. Should we make the controller an api server?” — agent-doc
<!-- agent-doc-subagent-response sha256=fe1802ea6b3711a453c38f3e6d861f5d785427f42663d199a1f43934dd00b049 -->
> **Subagent response:**
>
> Design only (no production code): branch design/automationapi, commit a1fff34641 "docs(proposal): language-agnostic automation API", new specs/proposals/automation-api.md (non-normative); make audit-docs exit 0.
> Recommendation: No - keep the controller as the authority behind the API, but do not publish its internal RPC. Today: local socket <root>/.agent-doc/controller.sock (agent-doc-controller/src/paths.rs:5,15), one newline JSON request/reply per connection (rpc.rs:21659-21707), 106 commands (rpc.rs:21843-22575) with 236 pub(crate) ControllerRequest literals, typed payloads as JSON strings in diagnostic_payload, prose errors (rpc.rs:22720), 50 rpc.rs commits in 30 days, exact binary-version match for writes (rpc.rs:22695; omitting the version skips it, rpc.rs:21823), no auth (specs/10-security.md:5), start-on-use/idle-retire/install-handoff lifecycle (08b:729-760); Kotlin and TS clients hand-write the JSON. command-plane-v1 (editors/command-payloads.md:1-31) already has the right shape; queue add (specs/07-orchestration-commands.md:1048) is the write path to copy; MCP (src/mcp.rs:362-441) is turn-oriented; 50 CLI --json subcommands (~3 with schema_version); agent-doc serve is an HTTP server with bearer tokens/TLS/SSE (src/serve.rs:1-40).
> Options: (a) versioned socket command (best authority fit, small socket client per language); (b) HTTP facade inside serve OK, gRPC conflicts with the no-async rule, a separate daemon would be a second authority; (c) CLI JSON (cheapest universal); (d) MCP should consume the API, not be it.
> Plan: one versioned agent-doc/api/v1 op table + JSON Schema with stable error codes in a pure agent-doc-api crate; phase 1 agent-doc api <op> --json (capabilities, documents.list, document.read, document.status, queue.add); phase 2 the same bytes as one controller socket command checked against the API version, with socket/dir permissions + SO_PEERCRED and Python/TS reference clients; phase 3 events.subscribe with a resumable document_version cursor across handoff; phase 4 optional /api/v1 + SSE in serve and generated MCP tools. v1 excludes document-text writes and run-a-turn.
> Open questions: first real caller; local-only vs remote/multi-user; is start-a-turn ever an API op; will automation write document text and which components; plain JSON vs Lazily CommandSubmit envelope; should API calls launch a controller and subscribers keep it alive; v1 stability/deprecation window; per-project endpoint vs user-level gateway.
> Follow-ups: (1) agent-doc serve POST /save with no open cycle writes the whole body with std::fs::write and commits (src/serve.rs:1135-1150), bypassing atomic_write_through_authority while an editor may hold the document; (2) lock down the controller socket (permissions, peer uid) and stop a request without binary_version from skipping the stale-binary refusal (rpc.rs:21823-21833); (3) specs/10-security.md:10 says no network services are exposed, but serve can bind non-loopback with bearer tokens.
<!-- agent-doc-subagent-completion key=8758a5a656d92cd060ca6e509199ab8fa4e3cf8858c793bf00fda3879a719e8e -->

### Re: Subagent completed: `subagent:jb263probeblind` returned work for queue item “do [#jb263probeblind]” — agent-doc
<!-- agent-doc-subagent-response sha256=479aaf5cc92e197a53e461708e9d3ba4688182991ca8cbbba481dc01777eed9b -->
> **Subagent response:**
>
> Branch fix/jb263probeblind, commit a99f5af7f7 on 7a92742ece (pushed); env -u TMUX -u TMUX_PANE make check exit 0, 11950 passed (186 skipped). No Kotlin changed.
> Root cause: agent-doc-fs/src/plugin_jar.rs probe_mapped_plugin_jar (~135) skipped every mapped file whose name does not parse as a versioned plugin jar; the 263 IDE maps only lib/modules/agent.doc.{backend,frontend,shared}.jar, so the probe returned Unknown and the editor route, the realtime remedy (superseded_editors=0) and the preflight warning all admitted silently. Also a jar still linked inside .agent-doc-jetbrains-install/backup-*/ was classified Current because the file exists.
> Rule (fold_mapped_plugin_jars): module jars are classified whenever the probe covers the modular line; a (deleted) module mapping or a still-linked mapping inside an installer backup-* tree is superseded (new MappedPluginJar::Displaced { path }); a superseded module jar outranks a current root jar (neither modular line upgrades restart-free); current module jars leave the probe Unknown (fail open, no advisory - the live IDE pid 3049656 maps exactly this shape); classic/262 root jars unchanged except a backup-linked root jar is Displaced; the restart verdict and preflight data-root lookups handle lib/modules paths. No marker/stale=true fallback needed.
> Files: agent-doc-fs/src/plugin_jar.rs, agent-doc-controller-io/src/project_controller/rpc.rs (editor_route_mapped_jar_probes(proc_root,...), Displaced in the gate), agent-doc-preflight-io/src/warnings.rs, src/plugin.rs (shared JETBRAINS_INSTALL_WORK_DIR), editors/jetbrains/SPEC.md, specs/07-session-tmux-commands.md, docs/reference/jetbrains-distribution.md.
> Tests: agent-doc-fs plugin_jar 10 passed (263 module-only deleted/backup-linked/replaced, current root next to deleted modules, classic-only stems blind, current 263 modules Unknown, 262/classic unchanged, restart verdict from lib/modules); controller-io superseded/exact_263 10 passed incl. exact_263_module_only_mapping_reaches_the_superseded_route_gate (advisory + editor_route_superseded_editor_admitted; none with current modules); preflight-io plugin 14 passed. Mutations (module scan off, backup check off, root wins, current modules surfacing, lib/modules marker lookup removed) each reddened.
> Note: leftover logs dir /home/brian/work/btakita/agent-doc-wt/jb263probeblind-logs.CSxB (rm -rf blocked by a hook).
<!-- agent-doc-subagent-completion key=d03289ee82372a2acf4c5bf2bf124e883567a808f38c6a807be5f31488814d0d -->

### Re: Subagent completed: `subagent:pmsync` returned work for queue item “So this is the first subtask.” — agent-doc
<!-- agent-doc-subagent-response sha256=4ed53c26fe9e32e8521b7c7b02c993d5644850ff094faaa895e1aabd01a9dcff -->
> **Subagent response:**
>
> One worker for the parent PM-sync request and its four nested subtasks. Branch feat/nestedsubprompts, commit d072914ecb (pushed, rebased on 7a92742ece); env -u TMUX -u TMUX_PANE make check exit 0, 11950 passed (pre-rebase; focused suites re-run green after). No editor sources changed.
> Part 1 (implemented, agent:run / agent:queue / agent:loop): a free-text row (live or struck) owns the nested rows below it as sub-prompts; the head text is parent + children verbatim and renders back byte for byte; children are never their own heads, claims or dispatch entries (parser and editor-node layer share one segmentation rule). Claim, node key, claim handle and tracked id derive from the parent line alone, so queue claim|start|release|brief --item '<parent line>' addresses the whole item. Editing or adding a child under a claimed parent is one edited change routed forward_to_owner to the parent's owner; the claim stays and nothing is re-dispatched. Lifecycle markers go on the parent line only (old per-child markers removed on the next marker update). Striking the parent retires the children ("- ~~parent~~" with children below). Edge cases: tracked children (do [#id], [#id], preset/dispatch lines) keep the old group shape; re [#id], [#id]: note, struck children, #gh-fix <url> and interior placeholders count as sub-prompt text; a do [#id] parent never takes sub-prompts; backlog/review/icebox/todo already treat nested rows as part of the item (tests confirm). Backward compatibility: nested free-text rows under a free-text row become sub-prompts (3 old tests rewritten). Promoting a free-text row to the backlog keeps children as the nested checklist and now strips lifecycle markers and [#owner#gen] annotations (the ☑️ [#movedsingleplugin] ☑️ [#subagent:...] leak).
> Files: agent-doc-element-queue/src/{subprompts.rs (new), lib.rs}, agent-doc-markdown-ast/src/{overlay.rs, mutations.rs}, agent-doc-queue/src/{document_queue.rs, queue_claim.rs, free_text_admission.rs}, agent-doc-session-check-io/src/midturn_steering.rs (test), tests/nested_subprompts_cli.rs (new), specs/02-document-format.md, specs/07-orchestration-commands.md, specs/pending-system.md, specs/proposals/pm-sync-integrations.md (new).
> Tests: agent-doc-queue lib 976 passed; nested_subprompts_cli 5 passed; session-check-io nested_subprompt 1 passed; 3 element-queue + 8 agent-doc-queue units; 7 mutations each fail a suite.
> Part 2 (design only): specs/proposals/pm-sync-integrations.md - gh:/gl:/jira: row identity, auth via existing gh login and passage-fetched GitLab/Jira tokens (document names only a profile), refresh via the controller scheduler, rate limits/conflicts, nested command rows for later writes, MVP GitHub read-only -> Jira -> GitLab -> writes. Open questions: one component per provider vs agent:tracker provider=...; credential profiles in project vs user config; refresh intervals and pause when no editor is open; commit tracker refreshes to git?; operator edit of a synced row (overwrite+log, treat as command, refuse visibly); command gesture and confirmation for destructive verbs; can a tracker row feed agent:run; meaning of "related to me".
> Follow-ups: (1) a do [#id] parent with nested free-text children still runs the children as heads; (2) the first projection write removing an old child marker may read once as an operator edit; (3) the live doc's four child rows carry stale [#subagent:pmsync#1] markers until the next marker update after install; (4) the backlog writer's prefix handling for the first ☑️ before the coined id is not traced.
<!-- agent-doc-subagent-completion key=fd8130d8efb918bf5d08cdac4f7268dcaf615917732a42839bd75a9d546435ca -->

### Re: Subagent completed: `subagent:pmsync` returned work for queue item “This is the second subtask.” — agent-doc
<!-- agent-doc-subagent-response sha256=4ed53c26fe9e32e8521b7c7b02c993d5644850ff094faaa895e1aabd01a9dcff -->
> **Subagent response:**
>
> One worker for the parent PM-sync request and its four nested subtasks. Branch feat/nestedsubprompts, commit d072914ecb (pushed, rebased on 7a92742ece); env -u TMUX -u TMUX_PANE make check exit 0, 11950 passed (pre-rebase; focused suites re-run green after). No editor sources changed.
> Part 1 (implemented, agent:run / agent:queue / agent:loop): a free-text row (live or struck) owns the nested rows below it as sub-prompts; the head text is parent + children verbatim and renders back byte for byte; children are never their own heads, claims or dispatch entries (parser and editor-node layer share one segmentation rule). Claim, node key, claim handle and tracked id derive from the parent line alone, so queue claim|start|release|brief --item '<parent line>' addresses the whole item. Editing or adding a child under a claimed parent is one edited change routed forward_to_owner to the parent's owner; the claim stays and nothing is re-dispatched. Lifecycle markers go on the parent line only (old per-child markers removed on the next marker update). Striking the parent retires the children ("- ~~parent~~" with children below). Edge cases: tracked children (do [#id], [#id], preset/dispatch lines) keep the old group shape; re [#id], [#id]: note, struck children, #gh-fix <url> and interior placeholders count as sub-prompt text; a do [#id] parent never takes sub-prompts; backlog/review/icebox/todo already treat nested rows as part of the item (tests confirm). Backward compatibility: nested free-text rows under a free-text row become sub-prompts (3 old tests rewritten). Promoting a free-text row to the backlog keeps children as the nested checklist and now strips lifecycle markers and [#owner#gen] annotations (the ☑️ [#movedsingleplugin] ☑️ [#subagent:...] leak).
> Files: agent-doc-element-queue/src/{subprompts.rs (new), lib.rs}, agent-doc-markdown-ast/src/{overlay.rs, mutations.rs}, agent-doc-queue/src/{document_queue.rs, queue_claim.rs, free_text_admission.rs}, agent-doc-session-check-io/src/midturn_steering.rs (test), tests/nested_subprompts_cli.rs (new), specs/02-document-format.md, specs/07-orchestration-commands.md, specs/pending-system.md, specs/proposals/pm-sync-integrations.md (new).
> Tests: agent-doc-queue lib 976 passed; nested_subprompts_cli 5 passed; session-check-io nested_subprompt 1 passed; 3 element-queue + 8 agent-doc-queue units; 7 mutations each fail a suite.
> Part 2 (design only): specs/proposals/pm-sync-integrations.md - gh:/gl:/jira: row identity, auth via existing gh login and passage-fetched GitLab/Jira tokens (document names only a profile), refresh via the controller scheduler, rate limits/conflicts, nested command rows for later writes, MVP GitHub read-only -> Jira -> GitLab -> writes. Open questions: one component per provider vs agent:tracker provider=...; credential profiles in project vs user config; refresh intervals and pause when no editor is open; commit tracker refreshes to git?; operator edit of a synced row (overwrite+log, treat as command, refuse visibly); command gesture and confirmation for destructive verbs; can a tracker row feed agent:run; meaning of "related to me".
> Follow-ups: (1) a do [#id] parent with nested free-text children still runs the children as heads; (2) the first projection write removing an old child marker may read once as an operator edit; (3) the live doc's four child rows carry stale [#subagent:pmsync#1] markers until the next marker update after install; (4) the backlog writer's prefix handling for the first ☑️ before the coined id is not traced.
<!-- agent-doc-subagent-completion key=feb43f02ce837792aa2b9f546e858fa32e26c09e676f9b39b458c453571c6acd -->

### Re: Subagent completed: `subagent:pmsync` returned work for queue item “Subtasks should be editable and steerable.” — agent-doc
<!-- agent-doc-subagent-response sha256=4ed53c26fe9e32e8521b7c7b02c993d5644850ff094faaa895e1aabd01a9dcff -->
> **Subagent response:**
>
> One worker for the parent PM-sync request and its four nested subtasks. Branch feat/nestedsubprompts, commit d072914ecb (pushed, rebased on 7a92742ece); env -u TMUX -u TMUX_PANE make check exit 0, 11950 passed (pre-rebase; focused suites re-run green after). No editor sources changed.
> Part 1 (implemented, agent:run / agent:queue / agent:loop): a free-text row (live or struck) owns the nested rows below it as sub-prompts; the head text is parent + children verbatim and renders back byte for byte; children are never their own heads, claims or dispatch entries (parser and editor-node layer share one segmentation rule). Claim, node key, claim handle and tracked id derive from the parent line alone, so queue claim|start|release|brief --item '<parent line>' addresses the whole item. Editing or adding a child under a claimed parent is one edited change routed forward_to_owner to the parent's owner; the claim stays and nothing is re-dispatched. Lifecycle markers go on the parent line only (old per-child markers removed on the next marker update). Striking the parent retires the children ("- ~~parent~~" with children below). Edge cases: tracked children (do [#id], [#id], preset/dispatch lines) keep the old group shape; re [#id], [#id]: note, struck children, #gh-fix <url> and interior placeholders count as sub-prompt text; a do [#id] parent never takes sub-prompts; backlog/review/icebox/todo already treat nested rows as part of the item (tests confirm). Backward compatibility: nested free-text rows under a free-text row become sub-prompts (3 old tests rewritten). Promoting a free-text row to the backlog keeps children as the nested checklist and now strips lifecycle markers and [#owner#gen] annotations (the ☑️ [#movedsingleplugin] ☑️ [#subagent:...] leak).
> Files: agent-doc-element-queue/src/{subprompts.rs (new), lib.rs}, agent-doc-markdown-ast/src/{overlay.rs, mutations.rs}, agent-doc-queue/src/{document_queue.rs, queue_claim.rs, free_text_admission.rs}, agent-doc-session-check-io/src/midturn_steering.rs (test), tests/nested_subprompts_cli.rs (new), specs/02-document-format.md, specs/07-orchestration-commands.md, specs/pending-system.md, specs/proposals/pm-sync-integrations.md (new).
> Tests: agent-doc-queue lib 976 passed; nested_subprompts_cli 5 passed; session-check-io nested_subprompt 1 passed; 3 element-queue + 8 agent-doc-queue units; 7 mutations each fail a suite.
> Part 2 (design only): specs/proposals/pm-sync-integrations.md - gh:/gl:/jira: row identity, auth via existing gh login and passage-fetched GitLab/Jira tokens (document names only a profile), refresh via the controller scheduler, rate limits/conflicts, nested command rows for later writes, MVP GitHub read-only -> Jira -> GitLab -> writes. Open questions: one component per provider vs agent:tracker provider=...; credential profiles in project vs user config; refresh intervals and pause when no editor is open; commit tracker refreshes to git?; operator edit of a synced row (overwrite+log, treat as command, refuse visibly); command gesture and confirmation for destructive verbs; can a tracker row feed agent:run; meaning of "related to me".
> Follow-ups: (1) a do [#id] parent with nested free-text children still runs the children as heads; (2) the first projection write removing an old child marker may read once as an operator edit; (3) the live doc's four child rows carry stale [#subagent:pmsync#1] markers until the next marker update after install; (4) the backlog writer's prefix handling for the first ☑️ before the coined id is not traced.
<!-- agent-doc-subagent-completion key=2674f9ad494da7505d9cfef3406913edcee869a18d5be78ecd7e97908dcba95a -->

### Re: Subagent completed: `subagent:pmsync` returned work for queue item “Make sure these subtasks are being deterministically associated with the parent task.” — agent-doc
<!-- agent-doc-subagent-response sha256=4ed53c26fe9e32e8521b7c7b02c993d5644850ff094faaa895e1aabd01a9dcff -->
> **Subagent response:**
>
> One worker for the parent PM-sync request and its four nested subtasks. Branch feat/nestedsubprompts, commit d072914ecb (pushed, rebased on 7a92742ece); env -u TMUX -u TMUX_PANE make check exit 0, 11950 passed (pre-rebase; focused suites re-run green after). No editor sources changed.
> Part 1 (implemented, agent:run / agent:queue / agent:loop): a free-text row (live or struck) owns the nested rows below it as sub-prompts; the head text is parent + children verbatim and renders back byte for byte; children are never their own heads, claims or dispatch entries (parser and editor-node layer share one segmentation rule). Claim, node key, claim handle and tracked id derive from the parent line alone, so queue claim|start|release|brief --item '<parent line>' addresses the whole item. Editing or adding a child under a claimed parent is one edited change routed forward_to_owner to the parent's owner; the claim stays and nothing is re-dispatched. Lifecycle markers go on the parent line only (old per-child markers removed on the next marker update). Striking the parent retires the children ("- ~~parent~~" with children below). Edge cases: tracked children (do [#id], [#id], preset/dispatch lines) keep the old group shape; re [#id], [#id]: note, struck children, #gh-fix <url> and interior placeholders count as sub-prompt text; a do [#id] parent never takes sub-prompts; backlog/review/icebox/todo already treat nested rows as part of the item (tests confirm). Backward compatibility: nested free-text rows under a free-text row become sub-prompts (3 old tests rewritten). Promoting a free-text row to the backlog keeps children as the nested checklist and now strips lifecycle markers and [#owner#gen] annotations (the ☑️ [#movedsingleplugin] ☑️ [#subagent:...] leak).
> Files: agent-doc-element-queue/src/{subprompts.rs (new), lib.rs}, agent-doc-markdown-ast/src/{overlay.rs, mutations.rs}, agent-doc-queue/src/{document_queue.rs, queue_claim.rs, free_text_admission.rs}, agent-doc-session-check-io/src/midturn_steering.rs (test), tests/nested_subprompts_cli.rs (new), specs/02-document-format.md, specs/07-orchestration-commands.md, specs/pending-system.md, specs/proposals/pm-sync-integrations.md (new).
> Tests: agent-doc-queue lib 976 passed; nested_subprompts_cli 5 passed; session-check-io nested_subprompt 1 passed; 3 element-queue + 8 agent-doc-queue units; 7 mutations each fail a suite.
> Part 2 (design only): specs/proposals/pm-sync-integrations.md - gh:/gl:/jira: row identity, auth via existing gh login and passage-fetched GitLab/Jira tokens (document names only a profile), refresh via the controller scheduler, rate limits/conflicts, nested command rows for later writes, MVP GitHub read-only -> Jira -> GitLab -> writes. Open questions: one component per provider vs agent:tracker provider=...; credential profiles in project vs user config; refresh intervals and pause when no editor is open; commit tracker refreshes to git?; operator edit of a synced row (overwrite+log, treat as command, refuse visibly); command gesture and confirmation for destructive verbs; can a tracker row feed agent:run; meaning of "related to me".
> Follow-ups: (1) a do [#id] parent with nested free-text children still runs the children as heads; (2) the first projection write removing an old child marker may read once as an operator edit; (3) the live doc's four child rows carry stale [#subagent:pmsync#1] markers until the next marker update after install; (4) the backlog writer's prefix handling for the first ☑️ before the coined id is not traced.
<!-- agent-doc-subagent-completion key=e2bc9c4c46ad5e5a7e94d8a5afe9e33995ae95f0df082886e751a0f94189f67e -->

### Re: Subagent completed: `subagent:clobberfollowups` returned work for queue item “do [#clobberfollowups]” — agent-doc
<!-- agent-doc-subagent-response sha256=92624189cbf3ff68fcac888e890e713f4e4da23635bd9ba10897311bab3a65da -->
> **Subagent response:**
>
> Branch fix/clobberfollowups, commit 8c376a1b49 on 7a92742ece (pushed); make check exit 0, 11950 passed (186 skipped); make tmux-ci exit 0, 207 passed. No Kotlin/TS.
> (1) The brief's hypothesis was wrong: the replay base and target were both correct (pending_write read live canonical ef39f88 = 3fc1e92 + merged operator typing 16:07:44-51; f22b45 is 128777 bytes vs 3fc1e92 129038). What was lost was a burst the editor reported at 16:07:53 before the CP write (editor_ops_recorded lineage_pre=ef39f88 lineage_post=b46c6bf) whose CRDT delta arrived after the 16:07:54 write and was StructuralQuarantined; no controller text held it, so OperatorTextWouldBeLost had nothing to compare. The write took the fast path (project_controller.rs, delivery.content == base_content -> ApplyTarget). Fix: new AwaitingEditorOps holds every projection of a retained target while the active op-capture epoch is based on exactly the visible text, has a non-empty reported tip differing from it, and its ops replay to that tip; on delta arrival the hold releases and the rebase rolls forward (or refuses). Test clobberfollowups_retained_target_waits_for_attested_editor_typing_then_rolls_forward.
> (2) commit_barrier_for_file_with_authority_and_delivery (agent-doc-crdt-relay-io/src/lib.rs) never checked quarantine (ready=true delivery_required=false), so after controller_commit_live_projection_wait_exhausted the local fallback committed 206af5eef3. Fix: barrier refuses while quarantined_live_replicas() is non-empty (crdt_commit_barrier_refused_quarantined_replica; commit_document and the local fallback both go through it); live_buffer_guard (agent-doc-git-io/src/live_buffer_guard.rs) refuses a ready barrier while the editor holds unmerged visible text; --force-disk still overrides. 2 tests.
> (3) All four realtime_doc_resolve sites (agent-doc-document-realtime-io/src/lib.rs) hard-coded diverged: false. CurrentText::Current gains quarantined_replicas (on the wire; old controller reads 0); crdt_relay_reconciliation (read_authority.rs) derives diverged; resolve and crdt_current_text logs print quarantined_replicas=. 3 tests.
> (4) New durable record agent-doc-controller-io/src/retained_operator_text_conflict.rs written on retained_transition_refused_operator_text_loss and cleared on the next retained projection; session-check reports INTERRUPTED ... state=retained_transition_refused_operator_text_loss with retained_operator_text_conflict_remedy (agent-doc-turn; resolve the conflicting lines, never re-send/force-disk/commit/recycle); mark_terminal_roll_forward_refusal records the terminal refusal without a prior exhausted record so the admin recycle remedy always renders. 5 tests + extended retainedprojectionclobber test.
> Files (22): SPEC.md, specs/14-realtime-workflow.md, specs/07-closeout-commands.md, agent-doc-controller-io/src/{project_controller.rs, project_controller/rpc.rs, retained_persistence_exhaustion.rs, retained_operator_text_conflict.rs, lib.rs}, agent-doc-crdt-relay-io/src/lib.rs, agent-doc-document-realtime/src/{read_authority.rs, lib.rs}, agent-doc-document-realtime-io/src/lib.rs, agent-doc-git-io/src/live_buffer_guard.rs, agent-doc-commit-io/src/lib.rs, agent-doc-session-check-io/src/command.rs, agent-doc-turn/src/write_ownership.rs, + new-field fixtures in preflight-io, route-io (2), start-io, write-converge-io, src/sim_world.rs. 11 mutations all caught.
> Follow-ups: (1) the direct CLI CP-write path (apply_canonical_replace_if_attached -> apply_cp_write_for_file_scoped) has no in-flight-typing hold; (2) the hold fires only while the epoch base is exactly the visible text, so bursts after an earlier delta in the same epoch landed are not held; (3) commit refusals reuse the generic "CRDT relay convergence is still pending" text (barrier returns only a bool over RPC).
<!-- agent-doc-subagent-completion key=50938ab607fc2053259a5a83981a9f665f7c64d466996c99bf8c80d101fab7e1 -->
> **Queue prompt:** I would like an agent-doc centric workflow to see all PR states + JIRA ticket states related to me or some other query synced in a document or element in an agent-doc. Note we need integrations with github + gitlab + Jira. Perhaps we can start with a dedicated tag to each integration? `agent:github` + `agent:gitlab` + `agent:jira`. Ideally it will be bidirectional. We can start with read-only syncing. But I would also like a way to send diff commands for each ticket. Perhaps we can have a nested list to send independent commands to avoid appending to a long line of text? This append subprompt behavior should apply to all lists such as `agent:run` + `agent:loop` + `agent:queue` + `agent:backlog` + `agent:todo` + `agent:review` + `agent:icebox`. `agent-exchange` lists would remain open to interpretation by the agent.
>   - So this is the first subtask.
>   - This is the second subtask.
>   - Subtasks should be editable and steerable.
>   - Make sure these subtasks are being deterministically associated with the parent task.

> **Queue prompt:** agent-doc should have an API for automation. This API should work across all languages. Should we make the controller an api server?

### Re: nested sub-prompts, PM-sync design, automation API, clobber follow-ups, 2026.3 probe — opus-5 · 2026-10-27T16:13-04:00

Four code fixes plus two design docs. Merged onto `main` (`7a92742ece..1959c54f63`, fast-forward) and installed locally: `#clobberfollowups`, `#jb263probeblind` and the nested sub-prompts change. Not released yet. On the merge, `make check` passed 11970/11970 and `make tmux-ci` passed 207, and no editor was quarantined at install time.

**Your feature request (with its nested subtasks).**
- **Part 1, implemented (`1959c54f63`):** in `agent:run`, `agent:queue` and `agent:loop`, the rows nested under a free-text row are its sub-prompts.
  - The parent and its children form **one** item. Claim, brief and dispatch are keyed by the parent line, and the children appear verbatim in the brief.
  - Editing or adding a child under a claimed parent goes to the parent's worker as `forward_to_owner`. Nothing is re-dispatched.
  - Lifecycle markers live on the parent only. Striking the parent closes the children.
  - Tracked children (`do [#id]`) keep the old behaviour.
  - It's already live in this document: your request now shows as one ☑️ item, the per-child markers are gone, and nothing is listed for separate dispatch.
  - The backlog marker leak (`☑️ [#movedsingleplugin] ☑️ …`) is fixed for future promotions.
- **Part 2, design only:** `specs/proposals/pm-sync-integrations.md`. The MVP order is GitHub read-only via `gh`, then Jira, then GitLab, then writes through nested command rows. Tokens come from `passage` and documents only name a profile. **Questions for you:**
  1. One component per provider, or `agent:tracker provider=…`?
  2. Where do credential profiles live?
  3. How often should it refresh, and should it pause when no editor is open?
  4. Should refreshes be committed to git?
  5. When you edit a synced row, should it overwrite, become a command, or refuse?
  6. What gesture sends a command, and do destructive verbs need confirmation?
  7. Can a tracker row feed `agent:run`?
  8. What does "related to me" mean?

**Automation API (your question): "should the controller be the API server?" No, not as the published protocol.** It should stay the authority behind the API. Its internal RPC is 106 crate-private commands, changes every week, requires an exact binary-version match, starts and retires on its own, and has no auth. **Recommendation** (`specs/proposals/automation-api.md` on `design/automationapi`, `a1fff34641`, not merged): one versioned `agent-doc/api/v1` operation table with JSON Schemas and stable error codes, in phases:
1. The CLI `agent-doc api <op> --json`: capabilities, documents.list, document.read, document.status, queue.add.
2. The same bytes as one controller socket command, versioned on the API, with same-user socket checks and small Python and TypeScript clients.
3. A resumable `events.subscribe`.
4. Optionally `/api/v1` in `agent-doc serve`, and generated MCP tools.

v1 has no document-text writes and no "run a turn now": automation queues work and the controller decides when it runs. **Questions for you:**
1. Who is the first caller?
2. Local-only, or remote?
3. Should "start a turn" be an operation?
4. Will automation ever write document text?
5. Plain JSON or the Lazily envelope?
6. Should API calls start a controller?
7. What stability promise for v1?
8. One endpoint per project, or one gateway?

**`#clobberfollowups` (`3bc326a398`).** The replay hadn't built its write from stale text. What was lost was a typing burst your editor reported at 16:07:53, one second before the write; its CRDT delta arrived after the write and was quarantined.
- A retained write now waits for editor typing that has been reported but not yet received.
- No commit, including the local fallback that committed `206af5eef3`, can seal a document while an editor is quarantined.
- `diverged` is now computed from the quarantine count.
- session-check reports a refused write, with the remedy.

**`#jb263probeblind` (`29911b1a9f`).** The 2026.3 IDE's module jars are now checked: a deleted or backup-path `lib/modules/agent.doc.*.jar` counts as a replaced plugin. Current module jars still allow the route through, so your live IDE gets no false advisory. The `#ahfe` live check should now show the full signal: the advisory plus `editor_route_superseded_editor_admitted`. `#ahfe` stays open for your check.

### Re: Subagent completed: `subagent:servesavebypass` returned work for queue item “do [#servesavebypass]” — agent-doc
<!-- agent-doc-subagent-response sha256=244bac968f0234ee615a3bcca5536a39ccf15d10932a25f7321d80d5166829c6 -->
> **Subagent response:**
>
> Branch fix/servesavebypass, commit 43734433ab on 1959c54f63 (pushed); env -u TMUX -u TMUX_PANE make check exit 0, 11973 passed (186 skipped).
> Root cause: src/serve.rs handle_save (~1135-1150) with no open cycle did std::fs::write(&doc, &body) then a plain git commit, never checking the editor authority or a live replica.
> Fix: one save_document path. Editor replica holds the document (crdt_authority_for_file(..).editor_attached(), the same check the direct-disk-write guard uses; unobservable controller counts as held) -> 409 Conflict with remedy "edit the document in that editor, or close it there, reload this page, and save again", nothing written or committed, an open cycle stays open, ops.log serve_save_refused reason=editor_replica_attached. No replica + no cycle -> atomic_write_through_authority then git commit. No replica + open cycle -> unchanged (atomic_write_through_authority + agent-doc write --commit). Refuse rather than merge: the browser posts a whole document built from a disk read with no base, so a merge keeps the browser side and drops operator typing (#retainedprojectionclobber class); routing through the authority alone returns a retained write (500) that would later push the browser text into the editor. /save and /save/<doc> both go through handle_save.
> Files: src/serve.rs, src/sim_world.rs (crate-visible SimEditor helpers), specs/07-core-commands.md (## serve), specs/10-security.md (browser editor writes; threat model now mentions agent-doc serve), SPEC.md.
> Tests (in-process, ephemeral loopback, SimEditor real replica): serve_post_save_writes_through_commit_boundary; serve_post_save_refuses_while_editor_replica_holds_document; serve_post_save_refuses_open_cycle_while_editor_replica_holds_document; serve filter 81/81 passed; 5 mutations all caught (M5 = original bug returned 200 and committed over the attached editor).
> Follow-ups: (1) serve GET /doc reads disk while an editor holds the document (should read the live document); (2) a based browser save that merges from the loaded text and refuses only on operator-line loss; (3) a small race between the editor check and the write/commit (recheck before committing).
<!-- agent-doc-subagent-completion key=8c80beab72db6d8e1e5c9034f4ed5f65624f3f607d8a25726691aeb010999ed5 -->

### Re: Subagent completed: `subagent:inflighttypinghold` returned work for queue item “do [#inflighttypinghold]” — agent-doc
<!-- agent-doc-subagent-response sha256=478864591af0b730b59ebd4563bc5340d6bc2ccdd4ed77341526e15d55008b0e -->
> **Subagent response:**
>
> Branch fix/inflighttypinghold, commit 66c136b4a9 (pushed); env -u TMUX -u TMUX_PANE make check exit 0, 11977 passed (186 skipped); make tmux-ci exit 0, 207 passed. No Kotlin/TS (no plugin calls the crdt_commit_barrier RPC).
> (1) apply_canonical_replace_if_attached (agent-doc-document-realtime-io/src/lib.rs ~3847; the single path to apply_cp_write_for_file_scoped for atomic writes, retained settles, projected captures, try_editor_converge) never read the editor's op-capture record. Now checks agent_doc_op_capture_io::editor_ops_in_flight_over before writing and waits in its retry loop (awaiting_editor_ops; {source}_crdt_write_held_for_editor_ops ... pending_from=K recovery=await_editor_delta_then_rebase once per burst); the existing rebase rolls forward once the delta lands; an unreadable record proceeds unheld (no deadlock).
> (2) editor_ops_in_flight_over_visible_cut (project_controller.rs ~8785) held only when the epoch base hash equalled the visible text. Shared rule agent_doc_merge::crdt::editor_ops_in_flight_over_visible (controller + CLI): hold when some tail of the epoch's ops replays from the visible text to the reported tip (whole-epoch still requires the base hash); never for lineage-less epochs, tip == visible, or non-replaying ops; shortest tails first with a 256 MiB work budget.
> (3) Relay barrier (agent-doc-crdt-relay-io/src/lib.rs ~5467) and RPC returned a bool. New agent_doc_turn::commit_barrier::CommitBarrierVerdict (ready | refused convergence_pending | quarantined_replica); RPC returns it only for {"verdict":true} requests (older clients get the bool; an older controller's bool reads as convergence_pending); live-buffer guard, flow-io closeout and finalize_commit render via live_buffer_guard::commit_barrier_refusal, printing state=crdt_commit_barrier_refused_quarantined_replica with quarantined_replica_commit_remedy.
> Files (18): agent-doc-merge/src/crdt.rs, agent-doc-op-capture-io/src/lib.rs, agent-doc-document-realtime-io/src/lib.rs, agent-doc-controller-io/src/project_controller.rs, .../rpc.rs, agent-doc-crdt-relay-io/src/lib.rs, agent-doc-git-io/src/live_buffer_guard.rs, agent-doc-commit-io/src/lib.rs, agent-doc-flow-io/src/closeout.rs, agent-doc-write-runtime-io/src/lib.rs, agent-doc-turn/src/{commit_barrier.rs (new), lib.rs, write_ownership.rs}, agent-doc-flow-io/tests/flow_closeout.rs, tests/test_cli.rs (guard forbids commit_barrier_verdict_for_file( outside the controller), specs/14-realtime-workflow.md, specs/07-closeout-commands.md, SPEC.md.
> Tests: deterministic hook-driven CLI hold+roll-forward (epoch base and mid-epoch), controller mid-epoch hold, 2 shared-rule units, verdict round-trip, RPC compat, relay verdict, guard quarantine message, flow-io closeout over a quarantined replica (quarantine message, HEAD unchanged); focused runs all pass; 8 mutations killed.
> Follow-ups: (1) the CLI hold adds one controller round-trip (document_state_projection) to most CP writes; (2) possible 60 s stall when this path runs inside the controller (e.g. compaction) with the editor delta queued behind the same request (safe refusal, unreproduced); (3) session-check does not yet report a quarantined replica as its own state.
<!-- agent-doc-subagent-completion key=bfeba7fa9d78b6656cfe2322c8d54de8027aa7ca92bbe11f706636ea7e9adb5c -->
x> **Queue prompt:** Add the questions as todo items. Use a nested list so each question is on its own line.

### Re: questions as nested Todos; serve /save, in-flight typing hold — opus-5 · 2026-10-27T17:55-04:00

**Done: both question sets are now Todos, with each question on its own nested line.**
- `#pmsyncquestions`: the PM-sync design questions.
- `#automationapiquestions`: the automation-API design questions.

That first needed a binary change. `--todo-add` refused multi-line text, so `feat/nestedtodoadd` (`dd9971e80e`) added nested sub-items:
- The first line is the Todo, and each `  - ` line is a sub-item, one level deep.
- Malformed child lines are refused, and nothing is applied.
- A replay is a no-op. `--todo-done` keeps the sub-items, and a multi-line `--todo-edit` replaces them.
- It also fixed a replay bug where a multi-line `--backlog-add` or `--icebox-add` filed a second copy or tripped the duplicate-id guard.

**Also on `main` and installed** (`1959c54f63..dd9971e80e`, not released). Every batch passed `make check` (last run 11987/11987) and `make tmux-ci` (207). Before each install I confirmed no editor was quarantined.

| Item | Commit | Fix |
|---|---|---|
| `#servesavebypass` | `23b71bd99c` | `agent-doc serve` `POST /save` now refuses with **409** while an editor replica holds the document, and nothing is written or committed. Otherwise it writes through the document authority instead of a raw `fs::write`. A merge wasn't safe: the browser posts a whole document with no base, so a merge would drop your typing. |
| `#inflighttypinghold` | `0154f75d3a` | The direct CLI write path now waits for editor typing that has been reported but not yet received. The hold also covers bursts after an earlier delta in the same epoch, using one rule shared by the CLI and the controller. A commit refused because an editor is quarantined now says so (`crdt_commit_barrier_refused_quarantined_replica`), instead of "convergence pending". |

### Re: follow-ups filed — opus-5 · 2026-10-27T18:02-04:00

I filed six follow-ups from this batch in the Icebox, and the Status sync block now shows main at `dd9971e80e` (installed, unreleased; latest release v0.35.557).
- `#nestedechostrike`: the pmsync and ahfe rows stay ☑️ because no command can close a reported subagent row. Their work is finished: pmsync Part 1 shipped and Part 2 is a design doc; the ahfe evidence pass is done, and its Todo waits on your live check.
- `#echosteermisread`, `#claimmarkersteer`: false steering. This cycle's "edited" report on the pmsync row was only the claim marker, so I did not forward anything to its finished worker.
- `#servegetdisk`, `#holdroundtrip`: follow-ups to `#servesavebypass` and `#inflighttypinghold`.
- `#nestedtodograndchildren`: `--todo-add` takes one level of nesting.
> **Queue prompt:** release + publish

> **Queue prompt:** [#ena0]: grok running

> **Chat prompt (#chatprompt:769519b380a6):** Is 558 installable by Coder?

### Re: v0.35.558 released and published; Restart Agent confirmed — opus-5 · 2026-10-27T20:08-04:00

**v0.35.558 is released and published** from release commit `00caa013ac` (on `main`). `make release` passed every phase:
- the tag handoff;
- the local `install-full`;
- public publication.

Release run 40602109793 succeeded in every job: all six builds, plugins, the GitHub release, PyPI and the Homebrew formula. Before the release, `make check` passed 11987 of 11987 tests, and I confirmed no editor was quarantined before the install.

**What's in it** (11 commits since v0.35.557; full notes in `VERSIONS.md`):
- A retained captured write rolls forward onto your text instead of replacing it (`#retainedprojectionclobber`, `#clobberfollowups`).
- Writes wait for in-flight editor typing, and commits refuse while an editor is quarantined (`#inflighttypinghold`).
- Restart Agent `--expect-agent`: it refuses, naming both agent values, when the controller's copy lags the editor.
- Nested sub-prompts in queues, and nested `--todo-add` sub-items.
- `serve` `POST /save` returns 409 while an editor holds the document (`#servesavebypass`).
- `compact` commits only the compacted cell; the remaining document reads resolve their store from the document; the 2026.3 module-jar plugin probe.
- Plugins: JetBrains classic 0.2.865, 262 0.2.867 and 263 0.2.866. VS Code is unchanged at 0.2.89.

**`#ena0` closed:** you confirmed `lazily.md` now starts Grok Build after Restart Agent.

**Is 558 installable by Coder? Yes.** v0.35.558 is GitHub's Latest release:
- PyPI has `agent_doc-0.35.558-py3-none-any.whl`.
- The release has Linux `x86_64-unknown-linux-gnu`, `x86_64-unknown-linux-musl` and `aarch64-unknown-linux-gnu` archives with `.sha256` files.
- The JetBrains Gateway backend plugin zips and the VS Code 0.2.89 VSIX are attached.

A workspace image that installs the latest agent-doc gets 558. One that pins a version needs the pin moved to 0.35.558. GH #380's harness-path fallback (0.35.557) is included, so a workspace without `~/.local/bin` on PATH still finds `claude`, `codex` or `grok`. I haven't run an install inside a live Coder workspace.
> **Queue prompt:** I would like an agent-doc centric workflow to see all PR states + JIRA ticket states related to me or some other query synced in a document or element in an agent-doc. Note we need integrations with github + gitlab + Jira. Perhaps we can start with a dedicated tag to each integration? `agent:github` + `agent:gitlab` + `agent:jira`. Ideally it will be bidirectional. We can start with read-only syncing. But I would also like a way to send diff commands for each ticket. Perhaps we can have a nested list to send independent commands to avoid appending to a long line of text? This append subprompt behavior should apply to all lists such as `agent:run` + `agent:loop` + `agent:queue` + `agent:backlog` + `agent:todo` + `agent:review` + `agent:icebox`. `agent-exchange` lists would remain open to interpretation by the agent.
>   - So this is the first subtask.
>   - This is the second subtask.
>   - Subtasks should be editable and steerable.
>   - Make sure these subtasks are being deterministically associated with the parent task.

> **Queue prompt:** sdk-demo.md subtask `  - Describe the choices in more detail. Why do we need a SBX/STG id?` is not in sync with the agent-doc state.

> **Queue prompt:** There are remaining ☑️ items in this queue.

> **Queue prompt:** [#automationapiquestions]: give me the full path to specs/proposals/automation-api.md if it's in a worktree

### Re: ☑️ rows closed, sdk-demo.md subtask sync, automation-API path — opus-5 · 2026-10-27T22:52-04:00

**The leftover ☑️ rows are closed.** I closed `do [#ahfe]` with the new `queue consume --ack-id ahfe`, which ends its reported claim and leaves the `#ahfe` Todo open for your live check. This echo closes the PM-sync row and its four sub-prompts: an echo of a nested row, one quoted line per row, now strikes the parent.

Both fixes are on agent-doc `main` (`00caa013ac..dd65b2dfa9`, fast-forward) and installed (`make install` exit 0, no JetBrains restart needed). Combined `make check`: 12004 of 12004 tests passed. I checked that no editor was quarantined before installing. Not released.

| Item | Commit | What changed |
|---|---|---|
| `sdk-demo.md` subtask out of sync | `ddc48d30e3` | The answer closed the struck parent and its child as one item, but only the parent was drawn struck, so the child looked open. The line you added later was folded into the closed block: no queue item, no steering, and no wake for the idle session until it was routed by hand. Now closing a row strikes its children too. A new line under a struck parent reopens the block as one live item, with the parent and the answered children as context, and it shows up as steering. The next preflight on `sdk-demo.md` strikes both of its children; both were answered in that session. |
| Leftover ☑️ rows (`#nestedechostrike`) | `dd65b2dfa9` | Each quoted echo line was compared against the whole multi-line head, so a nested parent could never strike. `--ack-id` accepted only backlog ids, never ended the claim, and never checked the claim state. Now a run of quoted lines that covers the whole head strikes it. `--ack-id` closes a dispatched row backed by a backlog item or a Todo, leaves the item open, and refuses work that is still running, lost, or owned by another session, naming the command to run instead. |

**`automation-api.md` path:** `/home/brian/work/btakita/agent-doc-wt/automationapi/specs/proposals/automation-api.md`. It had no worktree, so I created one on branch `design/automationapi` (`a1fff34641`). That branch is 9 commits behind `main` and isn't merged.

**Dispatched:**
- `do [#pmsyncquestions]`: a worker is writing your inline answers into `specs/proposals/pm-sync-integrations.md` as decisions, and answering your two questions back: a refresh recommendation that relies on deterministic change detection, and what happens when a status changes.
- The `agent:todo` sync row: a second worker is checking where Todo-backed rows differ from backlog-backed ones. This turn showed one: `queue brief` couldn't find `#pmsyncquestions` because it only searches backlog and review.

**Follow-ups from the workers:**
- If you strike a parent by hand and add a child before the next preflight runs, that child is still read as closed.
- The owner-session check on `--ack-id` can be bypassed by a plain shell that has no session env.
### Re: Subagent completed: `subagent:reportedrowclose` returned work for queue item “There are remaining ☑️ items in this queue.” — agent-doc
<!-- agent-doc-subagent-response sha256=7b05a46736bba0ae7f1e6d6c2e322e8d8542053625b4b64a1773299fca8d23c0 -->
> **Subagent response:**
>
> Branch fix/reportedrowclose, commit ddf12430c5 on 00caa013ac (pushed); env -u TMUX -u TMUX_PANE make check exit 0, 11995 passed (186 skipped).
> Root causes: (a) the strict echo matcher (agent-doc-queue/src/queue_response.rs) compared each quoted line alone against a multi-line nested head, so a nested parent echo never matched; (b) queue consume --ack-id accepted only open agent:backlog ids (ahfe is an agent:todo item), never ended the claim, and neither --ack-id nor --id checked the claim state.
> Fix: a contiguous run of quoted lines in one Queue prompt echo matches a nested head (one quoted line per head line); --ack-id accepts an open backlog or Todo id, requires dispatch proof for a Todo-backed row, refuses claimed/running/lost/other-owner claims naming the working command, and on success strikes the row, retires the completion cell and ends the reported claim.
> Files: agent-doc-queue/src/{queue_response.rs, queue_consume.rs}, agent-doc-queue-io/src/{queue_consume.rs, queue_cmd.rs}, src/main.rs, specs/07-orchestration-commands.md, runbooks/respond.md (+5 projected copies), tests/nested_subprompts_cli.rs.
> Tests: 8 new (2 queue, 4 queue-io, 2 CLI); reverting the fix fails all but the standalone owner-session helper test.
> Close commands: agent-doc queue consume FILE --ack-id ahfe; full nested echo in the respond closeout for the pmsync row.
<!-- agent-doc-subagent-completion key=09c7295348dd75b0c0b3b268ed30389f066e215307327de73c7214993d2d3869 -->
> **Queue prompt:** I would like an agent-doc centric workflow to see all PR states + JIRA ticket states related to me or some other query synced in a document or element in an agent-doc. Note we need integrations with github + gitlab + Jira. Perhaps we can start with a dedicated tag to each integration? `agent:github` + `agent:gitlab` + `agent:jira`. Ideally it will be bidirectional. We can start with read-only syncing. But I would also like a way to send diff commands for each ticket. Perhaps we can have a nested list to send independent commands to avoid appending to a long line of text? This append subprompt behavior should apply to all lists such as `agent:run` + `agent:loop` + `agent:queue` + `agent:backlog` + `agent:todo` + `agent:review` + `agent:icebox`. `agent-exchange` lists would remain open to interpretation by the agent.
>   - So this is the first subtask.
>   - This is the second subtask.
>   - Subtasks should be editable and steerable.
>   - Make sure these subtasks are being deterministically associated with the parent task.

> **Queue prompt:** sdk-demo.md subtask `  - Describe the choices in more detail. Why do we need a SBX/STG id?` is not in sync with the agent-doc state.

> **Queue prompt:** There are remaining ☑️ items in this queue.

> **Queue prompt:** [#automationapiquestions]: give me the full path to specs/proposals/automation-api.md if it's in a worktree
> **Queue prompt:** do [#pmsyncquestions]

### Re: PM-sync decisions recorded — opus-5 · 2026-10-27T23:20-04:00

**Your answers are now the design's decisions** in `specs/proposals/pm-sync-integrations.md` (`7f1bf60b7e`, on agent-doc `main`; docs only, `make check` 12004/12004). Nothing is built yet.

**Decisions:**
- **One component per provider:** `agent:github`, `agent:gitlab` and `agent:jira`. They share one network crate and one row shape.
- **Credential profiles:** the document names a profile only. Precedence is frontmatter > project config > global config. Frontmatter may set non-secret values (a Jira site, a GitLab host) and is refused for anything that looks like a token. Secrets still come from `passage`/`gh` at call time.
- **Refresh interval:** configurable (component `interval=` > frontmatter > project > global > default).
- **Commits:** only when the rendered rows change, in a commit of their own, never inside an agent turn's commit. Rows never show relative or clock times, so an unchanged poll stays byte-identical.
- **A row edit is a diffable command.** agent-doc compares your edit with the row it last rendered:
  - appended text becomes a command;
  - a changed field becomes the matching verb (a status edit becomes `transition:`, a title edit becomes `rename:`);
  - a deleted row hides the item; it never closes it.

  The command is saved as a nested sub-item tagged `(from row edit)`, so your intent is explicit to the agent, and the row re-renders from the provider. Nested sub-items stay the recommended way to add commands.
- **Commands follow queue rules:** a list, drained by `go`/`start`/`stop`, with the same per-line tags and with `subagents`/`subagents=N` supported. Destructive verbs need no extra per-row confirmation, but every command gets the stale-state check below.
- **Feeds:** `feed-run="<filter>"` and `feed-todo="<filter>"` send rows into `agent:run` and `agent:todo`, at most once per item per reason. When the provider shows the condition cleared, the fed Todo is checked off and an unstarted fed head is struck; a claimed head is left alone.
- **"Related to me" = `involves:@me`.** On GitHub that means author, assignee, mentioned or commenter. It doesn't include a review request on its own; see the first open point below.

**Your question: "what refresh rule do you recommend, given deterministic change detection?"**
- **Default:** `interval=5m`, with a 1-minute floor. A slower poll hint from the provider (GitHub's `X-Poll-Interval`) wins.
- **Each tick runs these checks in order,** going on only if the previous one reports a possible change:
  1. a conditional request (ETag). An authorized GitHub REST 304 doesn't count against the rate limit;
  2. an "updated since" cursor (Jira's has minute resolution, so each check overlaps the last by a minute and removes duplicates);
  3. a full re-fetch, whenever a change is reported and on every 6th tick. That is the only way to see items that dropped out of the query, such as a merged PR or a reassigned ticket;
  4. a digest of the rendered rows. If it's unchanged, nothing is written or committed.
- **Pause rule:** refresh only while the document is in use, meaning an editor has it open, a queue or loop is active, a subagent holds a claim, or a session owns the document. Otherwise it stops fetching. When the document comes back into use, one catch-up refresh runs, and preflight reports how old each component's rows are.
- GitHub's 304 and 5,000 requests/hour facts are verified. GitLab, Jira and GitHub search-ETag details are marked unverified in the doc.

**Your question: "what happens when a status is updated?"** The document only ever shows status the provider returned.
- **Status changes upstream:** the next refresh re-renders the row in one commit. Commands under the row are kept, and feeds re-evaluate.
- **Your command changes the status:** nothing changes as you type. Once the command is sent, the row re-renders from the provider's reply. If the provider rejects it, the command shows ⚠️ with the provider's reason, and the status stays as the provider has it.
- **Conflicts:** each command records the item's version when you wrote it, and agent-doc reads the item again just before sending:
  - if the item was closed or merged upstream, every unsent command on it is refused as stale;
  - state verbs (`transition:`, `close`, `reopen`) are refused if the state moved since you wrote them;
  - additive verbs (`comment:`, `label:`, `assign:`, `request-review:`) still send while the item is open;
  - a refused command stays in the list until you re-arm it with `go` or delete it.

I checked off `#pmsyncquestions` and added the six points the doc still has open as Todo `#pmsyncstillopen`, one per line.
### Re: Subagent completed: `subagent:inflighttypinghold` returned work for queue item “do [#inflighttypinghold]” — agent-doc
<!-- agent-doc-subagent-response sha256=478864591af0b730b59ebd4563bc5340d6bc2ccdd4ed77341526e15d55008b0e -->
> **Subagent response:**
>
> Branch fix/inflighttypinghold, commit 66c136b4a9 (pushed); env -u TMUX -u TMUX_PANE make check exit 0, 11977 passed (186 skipped); make tmux-ci exit 0, 207 passed. No Kotlin/TS (no plugin calls the crdt_commit_barrier RPC).
> (1) apply_canonical_replace_if_attached (agent-doc-document-realtime-io/src/lib.rs ~3847; the single path to apply_cp_write_for_file_scoped for atomic writes, retained settles, projected captures, try_editor_converge) never read the editor's op-capture record. Now checks agent_doc_op_capture_io::editor_ops_in_flight_over before writing and waits in its retry loop (awaiting_editor_ops; {source}_crdt_write_held_for_editor_ops ... pending_from=K recovery=await_editor_delta_then_rebase once per burst); the existing rebase rolls forward once the delta lands; an unreadable record proceeds unheld (no deadlock).
> (2) editor_ops_in_flight_over_visible_cut (project_controller.rs ~8785) held only when the epoch base hash equalled the visible text. Shared rule agent_doc_merge::crdt::editor_ops_in_flight_over_visible (controller + CLI): hold when some tail of the epoch's ops replays from the visible text to the reported tip (whole-epoch still requires the base hash); never for lineage-less epochs, tip == visible, or non-replaying ops; shortest tails first with a 256 MiB work budget.
> (3) Relay barrier (agent-doc-crdt-relay-io/src/lib.rs ~5467) and RPC returned a bool. New agent_doc_turn::commit_barrier::CommitBarrierVerdict (ready | refused convergence_pending | quarantined_replica); RPC returns it only for {"verdict":true} requests (older clients get the bool; an older controller's bool reads as convergence_pending); live-buffer guard, flow-io closeout and finalize_commit render via live_buffer_guard::commit_barrier_refusal, printing state=crdt_commit_barrier_refused_quarantined_replica with quarantined_replica_commit_remedy.
> Files (18): agent-doc-merge/src/crdt.rs, agent-doc-op-capture-io/src/lib.rs, agent-doc-document-realtime-io/src/lib.rs, agent-doc-controller-io/src/project_controller.rs, .../rpc.rs, agent-doc-crdt-relay-io/src/lib.rs, agent-doc-git-io/src/live_buffer_guard.rs, agent-doc-commit-io/src/lib.rs, agent-doc-flow-io/src/closeout.rs, agent-doc-write-runtime-io/src/lib.rs, agent-doc-turn/src/{commit_barrier.rs (new), lib.rs, write_ownership.rs}, agent-doc-flow-io/tests/flow_closeout.rs, tests/test_cli.rs (guard forbids commit_barrier_verdict_for_file( outside the controller), specs/14-realtime-workflow.md, specs/07-closeout-commands.md, SPEC.md.
> Tests: deterministic hook-driven CLI hold+roll-forward (epoch base and mid-epoch), controller mid-epoch hold, 2 shared-rule units, verdict round-trip, RPC compat, relay verdict, guard quarantine message, flow-io closeout over a quarantined replica (quarantine message, HEAD unchanged); focused runs all pass; 8 mutations killed.
> Follow-ups: (1) the CLI hold adds one controller round-trip (document_state_projection) to most CP writes; (2) possible 60 s stall when this path runs inside the controller (e.g. compaction) with the editor delta queued behind the same request (safe refusal, unreproduced); (3) session-check does not yet report a quarantined replica as its own state.
<!-- agent-doc-subagent-completion key=bfeba7fa9d78b6656cfe2322c8d54de8027aa7ca92bbe11f706636ea7e9adb5c -->
> **Queue prompt:** Add the questions as todo items. Use a nested list so each question is on its own line.
> **Queue prompt:** [#lazily-stale-cell-repair]: done

### Re: lazily stale completion cells; automation-API lazily questions dispatched — opus-5 · 2026-10-27T23:31-04:00

**`#lazily-stale-cell-repair` is checked off.** A fresh dry run of `agent-doc queue repair-stale-completion tasks/software/lazily.md` now reports **0** stale completion cells and **0** stale receipts (there were 14 on 2026-10-26).

**Dispatched:** your `automation-api.md` questions (use lazily's built-in features? extend lazily with endpoints? keep lazily as the multi-language API, or add a web API server, and if so does it belong in lazily?) went to a worker. It's reading the lazily specs and bindings and how agent-doc already uses lazily, and will answer each question with evidence. It will update `specs/proposals/automation-api.md` on `design/automationapi`, after rebasing it onto `main`; the full path is `/home/brian/work/btakita/agent-doc-wt/automationapi/specs/proposals/automation-api.md`.

`do [#pmsyncquestions]` is closed too. Its echo didn't strike the row, because the row is backed by a Todo, so I used `queue consume --id` after checking the Todo off. That's one more Todo-sync gap for the `agent:todo` worker that's still running.
> **Queue prompt:** `agent:todo` should always be in sync, like how `agent:backlog` is always in sync.
> **Queue prompt:** `agent:todo` should always be in sync, like how `agent:backlog` is always in sync.

### Re: Subagent completed: `subagent:todosync` returned work for queue item “`agent:todo` should always be in sync, like how `agent:backlog` is always in sync.” — agent-doc
<!-- agent-doc-subagent-response sha256=afb90279f7fa0c32df965182f23baf23d300b3c4e01720a39781f95a8d18cbce -->
> **Subagent response:**
>
> Branch feat/todosync, commit a4a550c3a4 on 7f1bf60b7e (pushed); env -u TMUX -u TMUX_PANE make check exit 0, 12011 passed (186 skipped).
> Gaps found vs backlog-backed rows: (1) queue brief ignored Todo-backed ids ("No backlog/review item ... carries #id"); (2) an operator-checked Todo left its do [#id] row live and dispatchable. Already at parity: --done, queue consume --ack-id, todo_signals.
> Fix: the brief falls back to the owning agent:todo item (component todo, action plus nested sub-items and replies verbatim; backlog wins on a shared id); queue maintenance strikes a live row once every Todo row owning that id is checked (source=todo_checked), never editing the Todo and skipping open backlog/review/icebox ids, operator-verdict heads and preset names. A drained row never checks its Todo off.
> Files: agent-doc-element/src/todo.rs, agent-doc-queue/src/subagent_brief.rs, agent-doc-preflight-io/src/lib.rs, specs/02-document-format.md, specs/07-orchestration-commands.md, tests/todo_queue_sync_cli.rs (new).
> Follow-ups: forward an edit to a referenced backlog item or Todo to its claimed worker; --todo-done strikes its do [#id] row in the same closeout; agent:todo priority does nothing; Todo same-id duplicates fold only after patch:todo or --todo-done.
<!-- agent-doc-subagent-completion key=4d32b85aba39c80f7a87d8e68360885cc6aa3b9d3dcd0151dca030239ace30f1 -->
> **Queue prompt:** `agent:todo` should always be in sync, like how `agent:backlog` is always in sync.

### Re: agent:todo kept in sync like agent:backlog — opus-5 · 2026-10-28T00:05-04:00

**Done: a `do [#id]` queue row backed by a Todo now stays in sync with that Todo, the way a backlog-backed row does** (`a4a550c3a4`, on agent-doc `main` and installed; `make check` 12011/12011; I checked that no editor was quarantined before installing; not released).

| Area | Backlog-backed row | Todo-backed row, before | Now |
|---|---|---|---|
| Worker brief | Quotes the backlog item | "No backlog/review item carries #id" (this is why I pasted `#pmsyncquestions` by hand) | Quotes the Todo, with every nested sub-item and reply verbatim |
| You check the item off | Row is struck | Row stayed live and could be dispatched again | Row is struck once every Todo row with that id is `[x]`; a reported claim ends with it |
| `--done <id>` | Closes both | Same | Same |
| `queue consume --ack-id` | Closes the row; item stays open | Same since `dd65b2dfa9` | Same |

**Rules kept:**
- A Todo is still yours: agent-doc never checks one off because a queue row finished, and never edits its text.
- Backlog wins when a backlog item and a Todo share an id.
- A Todo isn't mirrored into the queue, because a Todo is your action, not runnable work.

**Follow-ups (Icebox):**
- `#trackededitforward`: an edit to a backlog item or Todo that a running worker's row references isn't forwarded to that worker. This was never Todo-only; backlog has the same gap.
- `#tododonestrikerow`: `--todo-done` doesn't strike the matching `do [#id]` row in the same closeout; the next preflight does.
- `#todopriorityinert`: `agent:todo priority` sorts nothing.
- `#tododupefold`: Todo duplicates fold later than backlog duplicates do.

Still running: the worker on your `automation-api.md` lazily questions.

### Re: Subagent completed: `subagent:lazilyapi` returned work for queue item “automation-api.md - Can we use lazily's built-in features for this? - Should we extend lazily to provide api endpoints? - We have effective made lazily the api with different language bindings. Is this a good route or should we expand to a web-like api server? If yes, does the web server implementation belong in lazily?” — agent-doc
<!-- agent-doc-subagent-response sha256=726811297bfdefd991f3aa59d0dc264d731266157710f24401d888530eb1de5f -->
> **Subagent response:**
>
> Branch design/automationapi rebased onto 7f1bf60b7e and pushed (--force-with-lease); commits cc244309dc (proposal) + 7f0da2b144 (lazily placement, new section 5); docs only; make check exit 0, 12004 passed (186 skipped).
> (1) Use lazily features for semantics, not as a client requirement: public requests are plain JSON mapped 1:1 onto CommandSubmit and routed through the existing command plane (dedupe, stale-generation, handoff); writes return a CausalReceipt-shaped receipt; events.subscribe follows the boundary-ingress resume contract; status reads come from the controller state already kept. Not fit: raw state wire, byte-array CommandSubmit payloads for callers, tokio WebSocket transport, lazily permissions as auth.
> (2) No agent-doc endpoints in lazily (lazily owns envelope, product owns payload); three optional generic additions: per-binding local-socket command client, shipped resume logic, JSON-native payload form.
> (3) Keep lazily as the shared model and optional client toolkit; the API is agent-doc-owned and versioned, served CLI then local socket then HTTP/SSE in agent-doc serve; the web server belongs in agent-doc, not lazily.
> Follow-ups: agent-doc schemas/fixtures still in lazily-spec; command_plane_submit version-equality refusal must exempt the api command; Todo automationapiquestions should take revised q5 + new q9.
<!-- agent-doc-subagent-completion key=e7970f7ad9de97fb5bc9f1d36117f4738bd9a0f1433e8a8b7dfc834e6a2bd479 -->
> **Queue prompt:** automation-api.md
>   - Can we use lazily's built-in features for this?
>   - Should we extend lazily to provide api endpoints?
>   - We have effective made lazily the api with different language bindings. Is this a good route or should we expand to a web-like api server? If yes, does the web server implementation belong in lazily?

### Re: lazily and the automation API — opus-5 · 2026-10-28T00:40-04:00

The answers are in a new section 5 of `/home/brian/work/btakita/agent-doc-wt/automationapi/specs/proposals/automation-api.md`:
- branch `design/automationapi`, rebased onto `main`;
- commit `7f0da2b144`, docs only;
- `make check` passed 12004 of 12004 tests;
- not merged.

**1. Can we use lazily's built-in features? Yes, for how the API behaves, but callers shouldn't need lazily installed.**
- **Writes:** a request is plain JSON whose fields map one-to-one onto lazily's `CommandSubmit`. Inside the controller it becomes a real `CommandSubmit` on the existing command plane, so duplicate detection, the stale-generation check and surviving a controller handoff come from code that already exists. Each write returns a receipt shaped like lazily's `CausalReceipt`.
- **Events:** `events.subscribe` follows lazily's resume rules. Each event carries a generation and a cursor, a client resumes from its cursor, a gap triggers a replay, and a controller handoff starts a new generation.
- **Status reads:** they come from the lazily state the controller already keeps, so there's no second cache.
- **What doesn't fit:**
  - lazily's raw state wire, whose payloads are copies of internal structs;
  - asking callers for `CommandSubmit` directly, whose payloads are byte arrays plus a sha256;
  - lazily's WebSocket transport, which needs tokio;
  - lazily's permission model as authentication: it decides what a known peer may do, not who the peer is.
- **The bindings can't act as API clients yet:** none ships a socket command client or the resume logic.

**2. Should lazily be extended to provide API endpoints? No, not agent-doc's endpoints.** lazily's rule is that it owns the envelope and the product owns the payload, and `#lzsync` 3B already moved agent-doc's state view out of lazily. Endpoints in lazily would make every agent-doc API change a change across ten repositories. Three generic additions would be worth asking lazily for later; none of them blocks the first phases:
- a local-socket command client in each binding;
- a shipped version of the resume logic;
- a JSON-native payload form.

**3. Keep lazily as the API, or build a web API server, and where does it belong?** Keep lazily as the shared model and an optional client toolkit. The API should be agent-doc's own versioned set of operations, served first through the CLI, then the local socket, then HTTP + SSE in the existing `agent-doc serve`. That web server belongs in **agent-doc**, not lazily, and only once a real caller needs it.
- **Reach:** plain JSON works from every language, including shell and CI.
- **Security:** tokens, TLS, origin checks and socket credentials belong to agent-doc's transports; lazily has no authentication.
- **Stability:** the lazily envelope schema has barely changed since July, but lazily-rs went from 0.20 to 0.61 in under four months, and agent-doc pins different lazily versions per binding. A public API promise can't ride on that.
- "We've effectively made lazily the API" is right for the editor plugins, because they upgrade together with the binary. It's the wrong contract for outside callers.

**Changes to the plan:** question 5 (plain JSON or `CommandSubmit`) now has a recommended default for you to confirm: plain JSON, plus whether v1 should also accept raw `CommandSubmit`. There's a new question 9: should we ask lazily for the three client pieces? I haven't updated the `#automationapiquestions` Todo to match yet, because you're typing in the Todo and the duplicate-line bug below isn't fixed. I'll update it once that fix is installed.

**Follow-ups, filed in the Icebox:**
- `#lazilyspecagentdocfixtures`: agent-doc-specific schemas and fixtures still live in `lazily-spec`.
- `#apiversionexempt`: `command_plane_submit` refuses any caller whose binary version differs from the controller's, so the future `api` command must be exempt.

**Dispatched:**
- **The duplicated Todo line under `#pmsyncstillopen`:** the half-typed copy first appears in commit `28cda030f4`, my retried closeout, which landed while you were typing. The ops log shows a retained write scoped to the Todo landing at 03:16:42Z. A worker is tracing why that write carried your partial line, and fixing it so a retried write never puts an old snapshot of your typing back. You can delete the partial line yourself; I won't touch it.
- **`sdk-demo.md` ignoring `subagents`:** a worker is checking why its `agent:run priority subagents go` rows run inline, and fixing it so every path honors `subagents` or says loudly that it can't.
> **Chat prompt (#chatprompt:af080dce2a98):** Can you see the tmux layout when I open game-sessions.md?

### Re: tmux layout with game-sessions.md open — opus-5 · 2026-10-28T00:08-04:00

**Yes.** I read the live tmux server without changing anything. At about 04:05Z:

| Window | Panes |
|---|---|
| `0:0` agent-doc (visible) | `%6` agent-doc-bugs.md, 60 columns · `%16` game-sessions.md, 179 columns (active) |
| `0:1` stash | `%1` zsh, `%10` api.md, **`%17` sdk-demo.md (`sh`)**, **`%3` sdk-demo.md (`sh`)**, `%8` triage.md, `%11` sdk-design.md, `%2` lazily.md, `%4` frontend.md, `%12` sdk-pr.md, `%13` docs.md, `%7` infra.md |
| `2:0` grok | `%14` claude · `%15` grok |

Right now the visible window has two panes, but the split is 60 | 179 rather than an even editor split.

**Two panes in the stash window are titled `sdk-demo.md`, and both run `sh` instead of `agent-doc`.** That's my leading guess for your third pane.

haiven-dev's ops log also shows a layout sync failing around 03:59Z:
- it first saw a pane-count mismatch (2 columns wanted, 0 panes observed);
- then it kept retrying with `missing_tmux_session`;
- `sync_tmux_layout` exited with code 1 (`tmux_effect_retry_pending`).

**Dispatched:** your extraction report (all `project.md` presets copied into `game-sessions.md`, 3 tmux panes when you open it, and `sdk-demo.md` at half width in the agent-doc-bugs.md | sdk-demo.md split) went to a worker, along with this layout snapshot and the log lines. It may only read your tmux server, so any pane cleanup will come back to you as instructions rather than being done for you.

Still running: the duplicate-Todo-line fix and the `sdk-demo.md` `subagents` fix.
<!-- agent:boundary:72822783 -->
<!-- /agent:exchange -->

<!--
-->

## Run <!-- agent:run subagents preset="#spec-test-build-install-commit-push" priority go -->
- 🚧 [#subagent:typingdupe#1] todo item `- If the item is not in the list tree, agent-doc can add the item. If the item` is an agent-doc introduced duplicate that occurred while I was typing.
- 🚧 [#subagent:sdksubagents#1] sdk-demo.md `agent:run` has `subagents`, but subagents are not being used.
- 📥 do [#queue-blocked-marker]
- 🚧 [#subagent:extractissues#1] I extracted game-sessions.md from sdk-demo.md. There were some issues.
  - All project.md presets were copied over. This should not be the case. There should have been only preset overrides in the game-sessions.md frontmatter...which is probably no presets unless explicitly added.
  - When I open game-sessions.md in the editor, there are 3 tmux panes.
  - With the agent-doc-bugs.md|sdk-demo.md split, the sdk-demo.md tmux viewpord is half width.
- 📥 tmux sync failure: Sync failed: {"applied":false,"reason":"tmux_effect_retry_pending","columns":["/home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk/game-sessions.md","/home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk/sdk-demo.md"],"layout_generation":10,"layout_publisher":"command","window":null,"focus":"/home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk/game-sessions.md","no_autostart":false,"exact_visible":true,"routes_created_panes":true,"file_panes":[["/home/brian/work/btakita/agent-loop/src
<!-- /agent:run -->

## Loop <!-- agent:loop interval="1h" priority -->
- 📥 Watch https://github.com/btakita/agent-doc/issues for new issues. Add new issues into the backlog + queue except issue #40 + #45.
<!-- /agent:loop -->

## Todo <!-- agent:todo priority -->
- [ ] [#ahfe] Live-check GH #267 on the 263 IDE: after `agent-doc upgrade` WITHOUT restart, once idea.log shows `[native] hot-reloaded libagent_doc`, Run Agent Doc should complete (ops.log `editor_route_superseded_editor_admitted`, route output ends with the advisory line). 2026-10-15 operator: did not work (believed); watch for bug reports before reopening GH #267.
- [ ] [#ctg7] Under heavy load, switch the editor 2 panes -> 1 -> 2 and confirm tmux follows and stays at 2 (stale-controller fix).
- [ ] [#coder-remote-install-02805] Repair Coder Remote Dev sync after the two-sided install: open /home/brian/work/btakita/agent-loop as the backend IDE project root; ensure Client/Gateway and backend both load JetBrains 263 plugin 0.2.824; restart both; on the backend run agent-doc env --json, agent-doc tmux ensure tasks/agent-doc/agent-doc-bugs.md --ide-terminal --json, and agent-doc session status tasks/agent-doc/agent-doc-bugs.md; require client idea.log to show snapshot schema negotiated with geometry=measured, backend idea.log to show accepted ingress with no DELIVERY_FAILED, and .agent-doc/logs/ops.log to show controller_editor_surface_observed client=jetbrains-rd@... surface=remote-client:... followed by pane_layout_desired_published with column_order=editor. If DELIVERY_FAILED No such file or directory persists, fix ControllerSurfaceSnapshotPublisher to resolve the controller from each session document root instead of an unrelated IDE project basePath, expose connected-without-accepted-publication as blocked while preserving the no-guessing fence, and add a regression test.
- [ ] [#gh379-remotedev-proof] Close GH #379 (https://github.com/btakita/agent-doc/issues/379) after proof from its Remote Dev setup (Windows JetBrains Client): on that backend, `agent-doc --version` is 0.35.555 or newer and the IntelliJ IDEA 2026.3 plugin is 0.2.835 or newer; Run Agent Doc then ends exit 0 and ops.log shows `controller_editor_route_measured_layout_resolved` with no `editor route requires the exact visible layout before dispatch`.
- [ ] [#vscodecodersmoke] Run scripts/vscode-remote-smoke.py on a real Coder workspace (procedure in runbooks/coder-workspace.md): install, register, remote extension host, tmux attach, remoteName, layout sync; then update the VS Code on Coder status cell in editors/SPEC.md section 13.
- [ ] [#yeja] Choose how to handle ~/.agent-doc (see the #ddj8 answer in agent:exchange): (a) never treat $HOME as a project root, mirroring the tsift fix (icebox #homerootguard), (b) keep agent-doc activity in a child git repo such as ~/notes, (a)+(b) (recommended), or (c) status quo. Separately, ~/.agent-doc holds 1.8G (1.7G in the retired live-buffer/) that you can archive or delete. (source: agent:icebox [#homerootguard])
- [ ] [#automationapiquestions] Answer the automation-API design questions (specs/proposals/automation-api.md on branch design/automationapi)
  - Who is the first real caller: CI, a dashboard, another agent, or a bridge?
  - Local-only, or remote and multi-user (which needs a security spec first)?
  - Should "start a turn" ever be an API operation?
  - Will automation ever write document text, and which components could it own?
  - Plain JSON envelope, or the Lazily `CommandSubmit` envelope?
  - Should API calls launch a controller, and should event subscribers keep it from retiring?
  - What stability promise for v1, and how long a deprecation window?
  - One endpoint per project, or a user-level gateway across projects?
- [ ] [#pmsyncstillopen] Decide the PM-sync points still open (specs/proposals/pm-sync-integrations.md, "Still open")
  - Should the GitHub default also merge a second `review-requested:@me` query, since `involves:@me` misses review requests?
    - Yes
  - OK for a component `auth=` attribute to rank above frontmatter?
    - yes
  - Are the names `feed-run=` / `feed-todo=` and the filter list (`needs-me`, `review-requested`, `changes-requested`, `ci-failing`, `assigned`, Jira `status=`) right?
    - yes
  - What are the GitLab and Jira equivalents of `involves:@me`?
    - do the research
  - Where is a hidden (deleted) row stored, and how do you un-hide it?
    - Let's start with a list item that has sub list items with the operator using code folding. The list item can pick any title for the parent list item.
    - If the item is not in the list tree, agent-doc can add the item. If the item 
    - If the item is not in the list tree, agent-doc can add the item. If the item is in the list tree, the sync can match that item. Do a full walk.
  - Until provider idempotency keys are confirmed, is a local sent-command record plus a provider re-read before any retry acceptable?
    - yes
<!-- /agent:todo -->

## Backlog <!-- agent:backlog priority queue -->
- [ ] [#queue-blocked-marker] Give agent:run rows a distinct blocked marker (e.g. ⛔ blocked) and keep blocked rows out of subagent dispatch. Observed in tasks/sdk/sdk-demo.md on 2026-10-27: backlog items whose text carries '(stays open: blocked behind #a, #b)' (#template-api-create, #template-cli-create, #template-frontend-create, #bossroom-sbx-provision, #bossroom-stg-provision) render as 📥 pending, the same as runnable work, while one equally blocked item (#template-cli-catalog-digest) renders ⏭️ skipped, whose legend meaning ('dispatched and came back unconsumed') is wrong for it. preflight's queue_subagent_dispatch and session-check's 'queue awaits subagent dispatch' also offer those blocked heads every cycle, so the coordinator has to re-judge blockers by hand and the operator reads a queue of 📥 rows as idle capacity ('why is only 1 item being worked on with many inboxes?'). Expected: the binary derives blocked state from the stays-open blocker clause (and after= / unmerged prerequisite ids still open in backlog/review), renders a dedicated marker with the blocking ids, excludes those rows from queue_subagent_dispatch and drainable counts, and lists the legend entry in agent-doc queue legend and the status line.
<!-- /agent:backlog -->

## Review <!-- agent:review -->
- [/] [#jetbrainsmarketplaceprofile] [marketplace-approval] [agent-doc] Plugin 35436 is hidden and under initial JetBrains moderation; after approval, replace the basic description with polished copy, add Getting Started, Documentation, and Bugtracker URLs, then verify the public listing.
- [/] [#pypiquotarecheck] Track pypi/support#12214 (agent-doc project limit request, 25 GiB) to a decision. Unblocked mechanically by a human reply on pypi/support#12214, or make pypi-quota-check reporting a ceiling in reach. Rechecked 2026-10-12: pre-cutover 131 releases 8.361 GiB (0.35.113..0.35.343), post-cutover 91 releases 3.19 MiB (mean 35.9 KiB), about 47,722 further releases fit; VERDICT headroom fine. The support issue is still open with one comment, unchanged since 2026-09-10. Nothing to act on or delete. If the grant lands, record it and drop this item; deletion stays reserved for a ceiling the authenticated check reports.
- [/] [#gh299-live-263-proof] Live-verify v0.35.519 on JetBrains 263.7031 with changed plugin sources: `agent-doc upgrade` must hot-reload and complete without an IDE restart; close GH #299 only with that proof.
<!-- /agent:review -->

## Icebox <!-- agent:icebox -->
- [ ] [#lazilyspecagentdocfixtures] agent-doc-specific schemas and fixtures still live in lazily-spec (`schemas/agent-doc-state.json`, `conformance/agent-doc/`, the `editor_route` and `sync_tmux_layout` fixtures): move them into agent-doc or make them generic (automation-API proposal section 5).
- [ ] [#apiversionexempt] `command_plane_submit` refuses any caller whose binary version differs from the controller's; the future public `agent-doc api` command must be exempt or every install breaks automation clients (automation-API proposal, Phase 2).
- [ ] [#trackededitforward] An operator edit to a backlog item or Todo that a claimed queue row references is never forwarded to that worker (`forward_to_owner`); steering watches only exchange and queue edits (`#steerbacklogsource`). Affects backlog and Todo alike.
- [ ] [#tododonestrikerow] `--todo-done <id>` does not strike the matching `do [#id]` queue row in the same closeout the way `--done` does; the next preflight closes it (`source=todo_checked`).
- [ ] [#todopriorityinert] `<!-- agent:todo priority -->` does nothing: no pass sorts the Todo, and Todo `priority=` tokens do not rank `do [#todo]` rows under `agent:run priority`.
- [ ] [#tododupefold] Backlog drops identical same-id duplicates every preflight; the Todo folds them only after `patch:todo` or `--todo-done`.
- [ ] [#echosteermisread] Exchange steering reports the coordinator's own committed `> **Queue prompt:**` echo cells as new operator prompts, on almost every cycle.
- [ ] [#claimmarkersteer] Steering reports a claimed row as operator-edited (dispatch=forward_to_owner) when the only difference is the binary ☑️ claim marker plus nested rows that were there at claim time. It asks to forward to a worker that already finished.
- [ ] [#servegetdisk] serve follow-ups: GET /doc still reads disk, not the authority; a browser save carries no base revision, so it cannot be merged; the editor-attached check races the commit.
- [ ] [#holdroundtrip] The in-flight typing hold adds a controller round-trip to every direct CLI write and can stall up to 60s. session-check should report the quarantine state that the new commit refusal names.
- [ ] [#nestedtodograndchildren] `--todo-add` / `--todo-edit` accept one nesting level only; grandchildren and checkbox children are refused.
- [ ] [#controllersocketlock] From the automation-API design: lock down the controller socket (socket/dir permissions, SO_PEERCRED same-user check) and stop a request that omits binary_version from skipping the stale-binary refusal (agent-doc-controller-io rpc.rs:21823-21833); fix specs/10-security.md:10 ('no network services') now that agent-doc serve can bind non-loopback with bearer tokens. Also from nested sub-prompts: a do [#id] parent with nested free-text children still runs the children as separate heads.
- [ ] [#jbdocdrift] JetBrains doc drift found by #movedsingleplugin: editors/PLUGIN-SPEC.md:33-35 says two disjoint packages (there are three); PLUGIN-SPEC.md:74-79 keeps the old full module-graph replacement wording that contradicts the restated Exact-263 reopen rule (editors/jetbrains/SPEC.md:286-294,701-720); editors/jetbrains-262/README.md:114-118 says the resolver must not choose the exact-262 ZIP (it now picks the ranged ZIP). Also pending operator decision: merge 262+263 into one 262-263.* modular ZIP (option B). And VS Code Restart Agent needs the same --expect-agent check as JetBrains.
- [ ] [#cwdfallbacktail] Leftover cwd uses found by #cwdfallbackrest: (1) agent_doc_git_io::dirs::resolve_pane_cwd falls back to the process cwd for a document outside any git repo, and claim/route record it as the registry entry's cwd (should likely be the canonical parent); (2) agent-doc-sync ResyncTargetMatcher / resolve_absolute_file_path resolve relative registry entries against the process cwd (filter_registry_for_target, targeted prune) instead of the store; (3) terminal::run builds Tmux::default() instead of configured_tmux(), ignoring AGENT_DOC_TMUX_SOCKET and the configured tmux binary (also why terminal has no CLI test).
- [ ] [#compactrequester] From #compactabsorbsedit: controller_compact_document does not log who requested the compact (editor action, CLI, agent); add a requester= field. Also review whether operator edits inside the exchange before the boundary marker should still be archived and committed by compaction.
- [ ] [#orphandrainprobe] The orphan-drain liveness probe (supervisor_pid_for_doc) only matches 'start --route-owned', so a plain 'agent-doc start doc.md' supervisor (which runs an idle watch) reads as orphaned on every tick and only route-time deferral (GH #307, #autoclearorphandrain) prevents a double drive. Decide whether the drain should use start_supervisor_document_from_args (keeping kill paths route-owned only) and its effect on the GH #307 fallback. Also: tmux_session_policy_for_scope / sync_scope_root (agent-doc-sync/src/lib.rs:623) and route session_resolution.rs:41/223 read the cwd pin with no document (likely process/layout level).
- [ ] [#retireddirsgc] Other apparently retired directories under real .agent-doc stores (patches/, plugin-owner/, ack-content/, op-capture/; already forbidden by the hot-path guard) could join #livebuffergc's agent-doc gc --retired-stores list after the same retirement check. Also undecided (#queuerowdup): should a committed plain 'do [#id]' twin of a claimed row be collapsed or flagged (collapsing deletes operator-typed text)?
- [ ] [#homerootguard] Option (a) from the #ddj8 research, pending the operator's choice: never treat $HOME, / or the temp dir as an agent-doc project root (agent-doc-fs/src/lib.rs:44-61 find_project_root plus the JetBrains PatchWatcher.kt:288-297 / TurnStateBridge.kt:295 / CrdtReplicaManager.kt:5104 and VS Code extension.ts:318 root walkers, which also ignore the root ceiling) unless an override such as AGENT_DOC_ALLOW_HOME_ROOT=1 is set or ~/.git exists; move user-level logs to $XDG_STATE_HOME/agent-doc.
- [ ] [#rootlessstorecreate] Since #startcwdfallback, the first start of a document with no .agent-doc ancestor creates .agent-doc/ in the document's parent directory (as the bootstrap paths always did). Readers that skip rootless documents (turn-status, session-actor generation/log paths, start-runtime-io lib.rs:304/339, run.rs:196/404/1012) then find that store; decide whether a rootless store should be created at all or whether those readers should treat it consistently.
- [ ] [#citedworknag] Since #todoaddonsamecycledone, a follow-up todo created in the closeout that resolved its cited item is standalone, so preflight's cited_work_resolved advisory fires on every preflight for it (e.g. #krwr, #ddj8) even though it is outstanding by design. Quieting it needs a creation marker the document does not record. Also: claim still reads the process cwd's registry in find_alive_project_window (--window fallback) and the cwd config in project_tmux_session() (read-only; left to keep admission behavior).
- [ ] [#ownerlifecyclerevive] The owner lifecycle writer (store_lifecycle_actor_record_for_runtime callers, agent-doc-controller-io rpc.rs ~27253) decides from the runtime actor record and writes on generation only, so a same-generation Closed landing in that window could be overwritten by Ready. It is an owner write (kept generation-only by #controllerclosefence/#bindingcallersaudit); decide whether it should refuse to revive a Closed actor. Also unverified from #drainbusymask: a durable Busy with in-memory Ready would still block an admin drain (no writer found that produces it).
- [ ] [#evictdeadcode] evict_cross_document_pane_bindings (agent-doc-controller-io/src/project_controller.rs, fenced by #controllerclosefence) has no callers in the workspace; remove it or wire it.
- [ ] [#predispatchblockedaudit] After #predispatchreadyoverwrite, a Blocked or Closed record on the same pane, and a different-session record on the same pane, still get the pre-dispatch Ready rebind; audit whether a Blocked owner or the route's starting-timeout Blocked latch should be preserved. Related: after a stash rescue the record's window_id stays stale until sync's (now fenced, window-only) refresh repairs it.
- [ ] [#jbropeshadow] Make the JetBrains replica shadow a non-String text (rope or piece table) in CrdtReplicaManager and CrdtReplicaForwarder (knownReplicaText, the next flush base), so a flush stops allocating and copying the whole next shadow; that copy is now the only document-size-dependent part of prepareLocalEditorEditsUtil (about 25 us at 125 KB, 177 us at 500 KB). Gate on make jetbrains-keystroke-bench.
- [ ] [#jbfrontendswap] [agent-doc] Remote Dev frontend implementation swaps for the GH #339 shell design: let the JetBrains Client load the swappable implementation from the backend so frontend changes stop needing a restart. Parked 2026-10-26: it pushes backend code into the local client JVM without a plugin install, which changes the security boundary. The frontend stays shell-only (editors/jetbrains/SPEC.md, "Restart-free implementation generations").
- [ ] [#ghrecvpolicy] [agent-doc] Recipient-side [receive] auto_create_components policy for multi-user workspaces (GH #40, https://github.com/btakita/agent-doc/issues/40). Parked until multi-user workspace support exists.
<!-- /agent:icebox -->

## Done <!-- agent:done archive=tasks/agent-doc/agent-doc-bugs.done.md -->

<!-- completed work archived in tasks/agent-doc/agent-doc-bugs.done.md -->
<!-- /agent:done -->
