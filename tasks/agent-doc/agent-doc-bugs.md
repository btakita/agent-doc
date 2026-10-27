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
Released and published agent-doc v0.35.557 from release commit `fec25bdee1`; tag `v0.35.557` is the stable latest release (GitHub Latest with 24 assets, PyPI 0.35.557 public, `make release` public-publication complete) and is installed locally (install-full). JetBrains classic/262/263 0.2.862/0.2.863/0.2.864 (modular IDEs need a restart to load them), VS Code 0.2.89, Zed 0.1.2. GH #378, #380 and #381 closed; GH #379 stays open pending Remote Dev proof (todo `#gh379-remotedev-proof`). v0.35.557 carries the GH #380 harness install-dir fallback (supervisor and headless), GH #381 project-preset queue heads, queue claims that end on a strike, never cross queues and cancel on a same-slot replacement (`#krwr`), actor writes fenced on generation plus record revision, the queued-/clear running/pending snapshot fix, the orphan drain yielding to a supervisor mid re-exec, stores and config resolved from the document instead of the process cwd, `agent-doc commit` refusing outside git, `agent-doc gc --retired-stores`, the debug log honoring the root ceiling, GH #339 JetBrains shell stage R1, VS Code Remote Dev / koffi parity, Zed registration identity, and an edit-proportional native replica flush.
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
<!-- agent:boundary:a358906c -->
<!-- /agent:exchange -->

<!--
-->

## Run <!-- agent:run subagents preset="#spec-test-build-install-commit-push" priority go -->
- do [#cwdfallbackrest]
- do [#compactabsorbsedit]
- ~~do [#tracingrootceiling]~~
- ~~do [#commitoutsidegit]~~
- ~~do [#livebuffergc]~~
- ~~do [#autoclearorphandrain]~~
- ~~do [#loadprojectfordoccwd]~~
- ~~do [#queuerowdup]~~
- ~~[#krwr]: deleting a claimed queue row while adding a different row at the same position should cancel the claim instead of handing it to the new row as an edit~~
- ~~release + publish after "deleting a claimed queue row while adding a different row..." + [#autoclearorphandrain] [#loadprojectfordoccwd] [#queuerowdup] is integrated.~~
<!-- /agent:run -->

## Loop <!-- agent:loop interval="1h" priority -->
- 📥 Watch https://github.com/btakita/agent-doc/issues for new issues. Add new issues into the backlog + queue except issue #40 + #45.
<!-- /agent:loop -->

## Todo <!-- agent:todo priority -->
- [ ] [#ahfe] Live-check GH #267 on the 263 IDE: after `agent-doc upgrade` WITHOUT restart, once idea.log shows `[native] hot-reloaded libagent_doc`, Run Agent Doc should complete (ops.log `editor_route_superseded_editor_admitted`, route output ends with the advisory line). 2026-10-15 operator: did not work (believed); watch for bug reports before reopening GH #267.
- [ ] [#ctg7] Under heavy load, switch the editor 2 panes -> 1 -> 2 and confirm tmux follows and stays at 2 (stale-controller fix).
- [ ] [#coder-remote-install-02805] Repair Coder Remote Dev sync after the two-sided install: open /home/brian/work/btakita/agent-loop as the backend IDE project root; ensure Client/Gateway and backend both load JetBrains 263 plugin 0.2.824; restart both; on the backend run agent-doc env --json, agent-doc tmux ensure tasks/agent-doc/agent-doc-bugs.md --ide-terminal --json, and agent-doc session status tasks/agent-doc/agent-doc-bugs.md; require client idea.log to show snapshot schema negotiated with geometry=measured, backend idea.log to show accepted ingress with no DELIVERY_FAILED, and .agent-doc/logs/ops.log to show controller_editor_surface_observed client=jetbrains-rd@... surface=remote-client:... followed by pane_layout_desired_published with column_order=editor. If DELIVERY_FAILED No such file or directory persists, fix ControllerSurfaceSnapshotPublisher to resolve the controller from each session document root instead of an unrelated IDE project basePath, expose connected-without-accepted-publication as blocked while preserving the no-guessing fence, and add a regression test.
- [ ] [#gh379-remotedev-proof] Close GH #379 (https://github.com/btakita/agent-doc/issues/379) after proof from its Remote Dev setup (Windows JetBrains Client): on that backend, `agent-doc --version` is 0.35.555 or newer and the IntelliJ IDEA 2026.3 plugin is 0.2.835 or newer; Run Agent Doc then ends exit 0 and ops.log shows `controller_editor_route_measured_layout_resolved` with no `editor route requires the exact visible layout before dispatch`.
- [ ] [#lazily-stale-cell-repair] After the next tasks/software/lazily.md cycle commits it clean (no uncommitted edits, pane prompt_ready=true), run `agent-doc queue repair-stale-completion tasks/software/lazily.md` (dry run, 14 stale completion cells as of 2026-10-26), then `--apply` and `agent-doc commit tasks/software/lazily.md`.
- [ ] [#vscodecodersmoke] Run scripts/vscode-remote-smoke.py on a real Coder workspace (procedure in runbooks/coder-workspace.md): install, register, remote extension host, tmux attach, remoteName, layout sync; then update the VS Code on Coder status cell in editors/SPEC.md section 13.
- [ ] [#krwr] Decide whether deleting a claimed queue row while adding a different row at the same position should cancel the claim instead of handing it to the new row as an edit (current #queue-mutation-steering design; raised by the #claimfollowstruck fix).
- [x] [#ddj8] Decide whether ~/.agent-doc/ (which holds state.db) should make $HOME a project root: today any pane under $HOME with no closer .agent-doc/ resolves $HOME as its project root (found by #prunetestisolation; cf. the earlier ~/.tsift-made-$HOME-a-workspace fix).
- [ ] [#yeja] Choose how to handle ~/.agent-doc (see the #ddj8 answer in agent:exchange): (a) never treat $HOME as a project root, mirroring the tsift fix (icebox #homerootguard), (b) keep agent-doc activity in a child git repo such as ~/notes, (a)+(b) (recommended), or (c) status quo. Separately, ~/.agent-doc holds 1.8G (1.7G in the retired live-buffer/) that you can archive or delete. (source: agent:icebox [#homerootguard])
<!-- /agent:todo -->

## Backlog <!-- agent:backlog priority queue -->
- [ ] [#cwdfallbackrest] Remaining process-cwd fallbacks for a document with a known store (same class as #startcwdfallback / #claimwrongstore / #loadprojectfordoccwd): agent-doc-sync-io/src/resync.rs:267 resolve_registry_root falls back to current_dir() for a rootless target and resync.rs:380/:623 read the cwd's project_tmux_session(); src/terminal.rs reads the cwd registry (199) and cwd tmux_session pin (resolve_session_name, 306); claim's find_alive_project_window (--window fallback) reads the cwd registry; agent-doc-template-io load_project_from_doc (src/lib.rs:528-548) walks up for .agent-doc/config.toml ignoring the root ceiling. Resolve each from project_root_or_file_parent / ceiling-aware discovery.
- [ ] [#compactabsorbsedit] A compact exchange commit (controller_compact_document commit=true at 2026-10-27T12:48:49Z) absorbed an unanswered operator queue edit into the snapshot mid-turn, while a commit_document 57s earlier had refused the same drift as an unanswered edit; the absorbed rows then counted as intended repeats. Compaction's commit must apply the same unanswered-edit guard as commit_document (or leave operator-authored drift uncommitted).
<!-- /agent:backlog -->

## Review <!-- agent:review -->
- [/] [#jetbrainsmarketplaceprofile] [marketplace-approval] [agent-doc] Plugin 35436 is hidden and under initial JetBrains moderation; after approval, replace the basic description with polished copy, add Getting Started, Documentation, and Bugtracker URLs, then verify the public listing.
- [/] [#pypiquotarecheck] Track pypi/support#12214 (agent-doc project limit request, 25 GiB) to a decision. Unblocked mechanically by a human reply on pypi/support#12214, or make pypi-quota-check reporting a ceiling in reach. Rechecked 2026-10-12: pre-cutover 131 releases 8.361 GiB (0.35.113..0.35.343), post-cutover 91 releases 3.19 MiB (mean 35.9 KiB), about 47,722 further releases fit; VERDICT headroom fine. The support issue is still open with one comment, unchanged since 2026-09-10. Nothing to act on or delete. If the grant lands, record it and drop this item; deletion stays reserved for a ceiling the authenticated check reports.
- [/] [#gh299-live-263-proof] Live-verify v0.35.519 on JetBrains 263.7031 with changed plugin sources: `agent-doc upgrade` must hot-reload and complete without an IDE restart; close GH #299 only with that proof.
<!-- /agent:review -->

## Icebox <!-- agent:icebox -->
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
