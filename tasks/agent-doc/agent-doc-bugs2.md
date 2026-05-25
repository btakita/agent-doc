---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix.
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue_active: true
---

## Status

<!-- agent:status patch=replace -->
Fixed agent:queue stuck-after-1-turn — auto-strike done items at preflight.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260525-090813.md`*

Compacted content:
- Archived 3 response topic(s): do [#jbccc4]; do [#jbccc5]; do [#adoc-baseline-drift-after-user-commit]
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
### Re: do [#adoc-baseline-drift-after-user-commit] — opus-4-7

User-facing symptom fixed — `validate_replay` now auto-refreshes the capture baseline when the response body is still intact in the working tree, so the wedge no longer happens for the common "user committed something on top" workflow. Tracking the remaining depth-of-coverage work as separate items per the `#planperphase` recommendation.

#### What changed (binary)

- **`src/agent-doc/src/capture.rs:437` `validate_replay`** — previously bailed on exact SHA-256 mismatch of `file_hash` or `snapshot_hash`. Now, when both the file body and the captured `response_body` remain present in the current document (defers to `repair::response_already_applied` for normalized contiguous matching), the capture record's `file_hash` and `snapshot_hash` are refreshed to current and replay continues. The corruption-detection path is preserved: when the response body is rewritten or removed, the existing fail-closed message still fires.
- The refresh is logged as `capture_baseline_refreshed_for_benign_drift` so the recovery is auditable from `.agent-doc/logs/ops.log`.
- All flows that use `validate_replay` benefit transparently: `repair::run` (the primary call site at `src/repair.rs:1396`), `preflight::run`, and the `write --commit` repair gate.

#### Tests

- `validate_replay_refreshes_baseline_when_response_intact_after_user_commit` — new; pins the benign-drift recovery path.
- `validate_replay_still_bails_when_response_body_removed_from_current` — new; pins the corruption-detection path stays fail-closed.
- `validate_replay_rejects_diverged_file_hash` — existing test still passes (its "body changed" body doesn't contain the captured response body, so the benign-drift gate correctly returns false).
- `cargo test capture` and `cargo test validate_replay` both green.

#### Status of the bug

Marking the parent `#adoc-baseline-drift-after-user-commit` done — the wedge no longer occurs for the workflow the bug report described. Follow-up depth-of-coverage work split into per-phase items in `agent:backlog`:

- `#adoc-bdauc-simworld` — Phase 1 SimWorld scenarios.
- `#adoc-bdauc-preserve-session` — Phase 3 `agent-doc reset --from-current --preserve-session` non-destructive recovery.
- `#adoc-bdauc-runbook` — Phase 4 documentation.
- `#adoc-bdauc-integration` — Phase 5 in-source preflight integration test.

#### Closeout

- Submodule commit: `ccf3f51` — `fix: auto-refresh capture baseline on benign user-commit drift`.
- Superproject commit: `0500ccbe6` — `agent-doc: bump submodule for #adoc-bdauc Phase 2`.
- `cargo install --path .` + `cargo build --release` both ran; tmux + symlinked release binaries refreshed.

Plan: `tasks/agent-doc/plan-baseline-drift-after-user-commit.md`.
### Re: do [#adoc-prefix-strip-uncommitted] — opus-4-7

User-facing wedge closed — `validate_replay` now accepts the user-cleaned response body when a JB cache-conflict spill landed spurious `❯ ` markers and the user stripped them.

#### What changed (binary)

- **`src/agent-doc/src/repair.rs`** — new `pub(crate) fn response_already_applied_after_prefix_strip(doc, response)`. Mirrors `response_already_applied` but strips a single leading `❯ ` from each captured response line before the contiguous match. Non-prefixed response lines flow through unchanged so the helper degrades to the strict behavior when no spill happened.
- **`src/agent-doc/src/capture.rs`** — `validate_replay`'s benign-drift branch (added for `#adoc-baseline-drift-after-user-commit` Phase 2) now tries the strict `response_already_applied` first and falls back to the prefix-strip match. On either success the capture's `file_hash` / `snapshot_hash` are refreshed via the existing `capture_baseline_refreshed_for_benign_drift` event, so the recovery is auditable.

This implements the spirit of plan Phase 3 ("tighten the captured response contract — hash with the same normalization the file-IPC apply path uses, normalize prefix markers") without requiring a new `agent-doc repair --resync-capture` CLI surface (plan Phase 2). The recovery path is the existing implicit refresh; no new operator commands needed.

#### Tests

- `validate_replay_adopts_user_prefix_stripped_response_body` — new; pins the post-strip recovery path: captured body has a stray `❯ Submodule pointer updated.`, working tree has the cleaned `Submodule pointer updated.`, `validate_replay` returns Ok and the baseline refreshes.
- Existing `validate_replay_refreshes_baseline_when_response_intact_after_user_commit`, `validate_replay_still_bails_when_response_body_removed_from_current`, and `validate_replay_rejects_diverged_file_hash` all still green — the strict-match path is preserved.
- `cargo test validate_replay` green.

#### Scope notes

- Plan Phase 1 (session_check whitelist for `recovered_via_prefix_strip` distinct from `likely_direct_response_patchback`) and Phase 5 SimWorld coverage stay open. After this fix the failure mode that triggered Phase 1's concern no longer reaches session_check because validate_replay closes the cycle. If a regression shows session_check still misclassifying the pattern after this lands, that becomes a follow-up.
- Plan Phase 2 (`agent-doc repair --resync-capture` CLI) is not added; the implicit refresh covers the documented repro and avoids a destructive operator command surface.

#### Closeout

- Submodule commit: `ea62709` — `fix: adopt user prefix-stripped response in validate_replay`.
- Superproject commit: `f72e6e335` — `agent-doc: bump submodule for #adoc-prefix-strip-uncommitted`.
- `cargo install --path .` + `cargo build --release` both ran.

Plan: `tasks/agent-doc/plan-prefix-strip-uncommitted.md`.
❯ Can all the current features be ported to html using web components and the controller as a webserver? Is this architecture advisable or should there be a separate webserver?
### Re: web components + controller-as-webserver port — opus-4-7

Short answer: don't port wholesale; add a web frontend for specific use cases (remote, mobile, multi-viewer) and keep the editor-native path as the primary UI. If you do go web, prefer a separate webserver over folding HTTP into the controller — the controller is a deterministic state machine with one job per cycle and conflating it with a long-lived HTTP server invites lifecycle / lock-contention churn it currently avoids.

#### Why the current arch resists a wholesale port

- **"The document IS the UI"** is load-bearing. Users edit a markdown file in their existing editor (IntelliJ, VS Code, Vim, plain disk + git) and the binary reacts to those edits via IPC + file watching. A web-components UI replaces that with a new editing surface that has to *re-implement* editor semantics (cursor preservation, undo stacks, language modes, file-cache conflict resolution — the very pain points the JB cache-conflict bugs reflect).
- **The controller is per-cycle and per-document.** It does preflight → respond → finalize → commit, then exits. A controller-as-webserver would have to stay resident across cycles, share state across docs and clients, and serialize concurrent edits — that's a different system. Today's CLI also wins on debuggability: every step is reproducible from a shell prompt and lands a git commit.
- **Harness orchestration is tmux-shaped, not HTTP-shaped.** The supervisor/route/start/session-actor logic mediates Claude/Codex/OpenCode CLIs running in panes, owns pane lifecycle proof, and reconciles startup-miss / busy / blocked-stop states. Porting this to HTTP means either (a) reimplementing harnesses as long-lived HTTP services or (b) keeping a local agent under the webserver — which is what the current architecture already is, minus the browser layer.
- **Git as persistence + history** is already the source of truth. A web UI would still want to drive `git commit` on every closeout; nothing about HTML changes that boundary.

#### Where a web frontend would clearly help

- **Remote viewing of a live session** (read-only dashboard streaming the exchange + commits to a browser) — no editor port required, the controller can publish events over WebSockets or SSE.
- **Multi-viewer review** — pointing teammates at a URL to watch or comment without installing the JB plugin.
- **Mobile / casual visibility** — phone view of "what is my agent-doc loop doing right now."
- **Web components for the rendered exchange + backlog + queue + review state** — these are already structured components in the markdown; rendering them as HTML on the side is a straight projection, not a rewrite.

#### Architecture recommendation if you do this

- **Separate webserver, not controller-as-webserver.** Add a `agent-doc serve` (or sibling crate) that reads `.agent-doc/` artifacts (capture chain, cycle state, ops.log, snapshot/baseline) and exposes them over HTTP+WebSocket. Treat it as a *projection layer* over the existing on-disk state — read-only first, optional write-back via the same `agent-doc write --commit` / `finalize` boundary the editor IPC already uses. Lifecycle / locking / single-writer guarantees stay where they live today.
- **Web components consume the JSON projections preflight already emits.** preflight, plan, session-check all already produce structured JSON; web components rendering those is mostly a presentation layer, not new business logic.
- **Don't replace the editor IPC path.** Editor-native users keep file-IPC + socket IPC; web users get a parallel write surface that funnels through the same `write --commit` / `finalize` boundary so the cycle invariants (atomic write, snapshot, commit, session-check) hold uniformly.

#### Tradeoffs vs alternatives

- **Vs. controller-as-webserver:** lower coupling, no cross-cycle state sharing risk, but two processes to coordinate and a separate deploy story.
- **Vs. current editor-only path:** more code to maintain, web auth/CSP surface to think about, but unlocks remote/mobile/multi-viewer cases that JB/VS Code plugins cannot.
- **Vs. doing nothing:** if the dominant pain is "I want to see this from my phone," a tiny SSE-only read-only viewer captures most of the value for ~1% of the engineering surface a full web rewrite would need.

❯ Want me to draft a focused plan for the projection-layer `agent-doc serve` if that direction sounds right?
❯ I would like to have a fully featured web interface. It can be a markdown editor for now.
### Re: web interface MVP — opus-4-7

`agent-doc serve <FILE>` now boots a localhost HTTP markdown editor on `http://127.0.0.1:7333/` (override with `--host` / `--port`). Phase 1 of the web-interface plan is in HEAD; phases 2–6 split into per-phase backlog items so each can be picked up independently.

#### How to try it

```
agent-doc serve tasks/agent-doc/agent-doc-bugs2.md
# then open http://127.0.0.1:7333/ in a browser
```

Edit the markdown, hit save (or Ctrl+S). The response payload returns the new HEAD short SHA so you can confirm the commit landed without leaving the browser.

#### What's in the MVP

- **`src/agent-doc/src/serve.rs`** — `tiny_http`-backed sync server (no async; project rule). Routes: `GET /` (embedded HTML editor), `GET /healthz`, `GET /doc` (current text), `POST /save` (full-document replace → `git add` + `git commit --no-verify` with generated message).
- **`src/agent-doc/assets/serve/index.html`** — embedded via `include_str!`. Raw `<textarea>` editor, dark theme, Ctrl+S save, dirty-state indicator, HEAD readout in the footer, reload-from-disk button.
- **`src/agent-doc/Cargo.toml`** — adds `tiny_http = "0.12"`. Chosen for the sync-only constraint (rules out tokio/axum); ~15 KB compiled, no transitive runtime.
- **`src/agent-doc/src/main.rs`** — wires `agent-doc serve <FILE> [--host] [--port]` subcommand.

#### Deliberate scope decisions

- **User edits commit through plain `git`, not `write --commit`.** The cycle/capture machinery is reserved for agent responses through `write --commit` / `finalize`. Web-editor user edits are explicitly outside the cycle boundary for Phase 1 — Phase 6 will route writes through the cycle when an active cycle is in scope, matching the editor IPC contract.
- **Localhost-only bind by default.** Non-loopback host prints a clear warning; auth lands in Phase 5.
- **Raw `<textarea>` editor.** Phase 3 swaps in CodeMirror 6 with proper markdown mode + side panels for backlog/queue/review web components.
- **No SSE.** Phase 2 adds `/events` with `notify`-backed file watching so external mutations (editor IPC, the CLI, another writer) push to the browser.

#### Smoke test

End-to-end manual test against a temp git repo:
- `GET /doc` returned the seed `# hello`.
- `POST /save` with body `# hello from web edit` returned `{"ok":true,"head":"5863939","bytes":21}`.
- `git log --oneline` showed the new commit landed (`5863939 agent-doc serve: web edit <timestamp>`).
- Subsequent `GET /doc` reflected the new content.

#### Follow-ups added to backlog

- `#adoc-web-sse` — Phase 2 SSE updates.
- `#adoc-web-codemirror` — Phase 3 editor swap + JSON-projection components.
- `#adoc-web-multidoc` — Phase 4 root mode + session list.
- `#adoc-web-auth` — Phase 5 token auth + TLS.
- `#adoc-web-live-cycle` — Phase 6 stream integration.

#### Closeout

- Submodule commit: `(see superproject pointer bump)` — `feat: agent-doc serve — localhost markdown editor (Phase 1 MVP)`.
- Superproject commit: `01280713e` — `agent-doc: add web interface plan + Phase 1 MVP submodule bump`.
- `cargo install --path .` + `cargo build --release` both ran; tmux `~/.cargo/bin/agent-doc` and `.bin/agent-doc → target/release/agent-doc` carry the new subcommand.
- Plan: `tasks/agent-doc/plan-web-interface.md`.
<!-- agent:boundary:7b944553 -->
<!-- /agent:exchange -->
###
<!--
As I was typing into the comment below `/agent:exchange`, the full-document IPC corruption happened, then the duplicate line happened, then the full-document IPC corruption happened again.
#spec-test-build-install-commit-push
---
JB `Restart Supervisor Process` error:
```
agent-doc command failed (exit 1): Error: session_restart refused for /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md because the authoritative actor is still starting and the document changed after the last committed cycle. Wait for a dispatch-ready prompt (`prompt_ready=true`) and retry, or run `agent-doc session status /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md` to inspect the pane.
```
-->

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- ~do [#crdtrespfx]~
- ~do [#queueadvgated]~
- ~do [#jbrsrbusysim]~
- ~do [#cclooploop]~
- ~do [#ipcdupfin23]~
- ~do [#ccloopguard]~
- ~do [#p6skillinstallreconcile]~
- ~do [#agent-doc-commit-sibling]~
- ~do [#adcs-binary]~
- ~do [#adoc-jb-cache-cancel-stuck-cycle]~
- ~do [#jbccc1]~
- ~do [#jbccc2]~
- ~do [#adoc-compact-during-turn-response-loss]~
- ~do [#jbccc3]~
- ~do [#jbccc4]~
- ~do [#jbccc5]~
- ~do [#adoc-baseline-drift-after-user-commit]~
- ~do [#adoc-prefix-strip-uncommitted]~
- do [#adoc-bdauc-simworld]
- do [#adoc-bdauc-preserve-session]
- do [#adoc-bdauc-runbook]
- do [#adoc-bdauc-integration]
- do [#jbrsrbusysw]
- do [#jbrsrbusyide]
- do [#jbccfix]
- do [#jbccreg]
- do [#jbccdocs]
- do [#planperphase]
<!-- /agent:queue -->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#adoc-web-sse] Phase 2 of agent-doc serve: SSE /events stream + frontend reload on file change. Reuse the existing notify integration if available, else 250-500ms poll. Plan: tasks/agent-doc/plan-web-interface.md (Phase 2).
- [ ] [#adoc-web-codemirror] Phase 3: swap the raw textarea for CodeMirror 6 with markdown mode, line wrap, gutter; render exchange/backlog/queue/review as web components consuming preflight JSON projections. Plan: tasks/agent-doc/plan-web-interface.md (Phase 3).
- [ ] [#adoc-web-multidoc] Phase 4: root mode + session list (/api/sessions reuses sessions.json), URL routing /doc/<path-or-hash>. Plan: tasks/agent-doc/plan-web-interface.md (Phase 4).
- [ ] [#adoc-web-auth] Phase 5: bearer token auth for non-loopback binds, optional TLS via rustls, multi-user read-only viewer scope. Plan: tasks/agent-doc/plan-web-interface.md (Phase 5).
- [ ] [#adoc-web-live-cycle] Phase 6: hook agent-doc stream into /events so the in-flight ### Re: block streams to the browser as the agent produces it; web writes funnel through finalize/write --commit when an active cycle is in scope. Plan: tasks/agent-doc/plan-web-interface.md (Phase 6).
- [ ] [#adoc-prefix-strip-session-check-whitelist] Phase 1 of #adoc-prefix-strip-uncommitted: session_check.detect_bypassed_response_write false-positives on legitimate response prose lines (treats them as 'bare prompt target missing ❯ '). When a fresh preflight_started cycle just rotated mid-turn and has a visible response body but no capture/pending, the heuristic still flags the response as a likely_direct_response_patchback. Need to recognize and whitelist this shape (e.g., when the diff insert is bracketed by a legit ### Re: heading and the prose differs only by the absence of ❯ prefixes). Demonstrated live during the #adoc-prefix-strip-uncommitted cycle closeout — required bare git commit + reset --from-current fallback. Plan: tasks/agent-doc/plan-prefix-strip-uncommitted.md (Phase 1).
- [ ] [#adoc-bdauc-simworld] SimWorld scenario coverage for #adoc-baseline-drift-after-user-commit Phase 1: model the three drift branches (benign outside-response, inside-response-mismatch, inside-response-matches-normalized) in src/sim_world.rs. Plan: tasks/agent-doc/plan-baseline-drift-after-user-commit.md (Phase 1).
- [ ] [#adoc-bdauc-preserve-session] agent-doc reset --from-current --preserve-session: non-destructive recovery that rebuilds the baseline but keeps session id, snapshot history, and capture chain. Today's --from-current is destructive. Plan: tasks/agent-doc/plan-baseline-drift-after-user-commit.md (Phase 3).
- [ ] [#adoc-bdauc-runbook] Document baseline-drift recovery: new runbooks/baseline-drift.md (or extend jb-cache-conflict.md cross-link), SKILL.md catalog mirrors, runbooks/commit.md reference. Plan: tasks/agent-doc/plan-baseline-drift-after-user-commit.md (Phase 4).
- [ ] [#adoc-bdauc-integration] In-source preflight integration test mirroring preflight_auto_recovers_jb_cache_conflict_cancel_committed_with_snapshot_drift: real git repo + cycle state, simulate user-committed benign drift, assert validate_replay auto-refreshes and preflight proceeds. Plan: tasks/agent-doc/plan-baseline-drift-after-user-commit.md (Phase 5).
- [ ] [#jbrsrbusysw] SimWorld scenario coverage for session_restart --force: route + Restart Supervisor while pane is alive-busy with and without --force, asserting the new refusal message and force success (including session_restart_busy_pre_interrupt_idle and session_restart_busy_force_killed event variants). Substantial — requires new Op variant + tmux/session_actor mock in src/sim_world.rs. Plan: tasks/agent-doc/plan-jb-restart-supervisor-busy-refusal.md phase 4 (SimWorld piece).
- [ ] [#jbrsrbusyide] JB plugin IDE-side telemetry for session_restart --force: surface session_restart_force_used, session_restart_busy_pre_interrupt_idle, and session_restart_busy_force_killed events in the IntelliJ notification area or Logcat so users can see the recovery path the binary took. Kotlin work in editors/jetbrains/. Plan: tasks/agent-doc/plan-jb-restart-supervisor-busy-refusal.md phase 4 (IDE piece).
- [ ] [#planperphase] Convention: split multi-phase plans into per-phase backlog IDs (e.g. #crdtrespfx1, #crdtrespfx2) instead of one parent ID that gets pending-gated. Lets queue auto-strike honor per-phase progress. Lower priority — the convention is opt-in for authors. [recommended]
- [ ] [#jbccfix] Fix the Compact Exchange write/cache boundary so JetBrains never receives a stale or conflicting file-cache snapshot after compact exchange.
- [ ] [#jbccreg] Add deterministic regression coverage for Compact Exchange cache-conflict handling, including stale snapshot and concurrent user-edit cases where feasible.
- [ ] [#jbccdocs] Document the Compact Exchange File Cache Conflict recovery path in the JetBrains/harness runbooks after the behavior is fixed.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#jbccc2] Phase 2 of #adoc-jb-cache-cancel-stuck-cycle: JB plugin undo-on-cancel. editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/PatchWatcher.kt — defer the write until user resolves the File Cache Conflict dialog (never queue change beforehand); on Cancel return 0/2 from socket IPC callback; on file-IPC cancel leave patch file in place. Plan: tasks/agent-doc/plan-jb-cache-cancel-stuck-cycle.md
- [/] [#agent-doc-commit-sibling] agent-doc CLI: add --commit-sibling <repo> mode to finalize/write. When the session doc lives in repo A but the user's directive originates a commit in sibling repo B (e.g. session-share session-doc drives a build-party scripts change), agent-doc should be able to stage and commit the sibling repo with a Session-Doc git trailer auto-injected. Trailer format: 'Session-Doc: https://github.com/<session-share-org>/<repo>/blob/<session-share-cycle-sha>/<session-doc-path>'. Today agent-doc only commits inside the session-doc's own submodule, so the agent (Claude) creates sibling commits manually via 'git commit' and the trailer is easy to forget. Companion item #cross-repo-sessiondoc-trailer in tasks/docs.md covers the convention + backfill-updates.sh changes across the four script copies. Plan path: tasks/agent-doc/plan-commit-sibling.md
- [/] [#adoc-jb-cache-cancel-stuck-cycle] agent-doc-bug: after canceling a JetBrains File Cache Conflict dialog during IPC patch delivery, the response patch lands in the working tree but the cycle does not reach committed state. Recovery requires running agent-doc write --commit FILE manually. Repro: have JB IDE open with the file in an editor; run finalize; cancel the JB cache-conflict dialog when it appears. Expected: cycle reaches committed cleanly or surfaces a clear error. Actual: response is in working tree but next preflight reports post-commit local drift requiring manual recovery. Plan path: tasks/agent-doc/plan-jb-cache-cancel-stuck-cycle.md (to be authored). Related: #jbccfix, #jbccreg, #jbccdocs in this same file.
- [/] [#jbrsrbusy] [partial] Phase 1 (binary) landed: agent-doc session restart <FILE> --force flag accepts alive-busy panes, refusal message now ends with the --force hint citing the exact command, and the force path logs session_restart_force_used. Plan phases 2 (JB plugin confirm dialog → invokes --force), 3 (optional harness-aware pre-interrupt with 2s idle wait), and 4 (SimWorld coverage + IDE-side telemetry) split into #jbrsrbusyjb, #jbrsrbusyint, and #jbrsrbusysim. Plan: tasks/agent-doc/plan-jb-restart-supervisor-busy-refusal.md.
- [/] [#stuckcycledup] [recommended] [partial] Wedged-cycle pattern: cycle_already_committed gate now refuses the short-circuit when the active capture body is absent from HEAD (flow/closeout.rs::cycle_already_committed + new stuck_captured_cycle helper), and preflight emits a stuck_captured_cycle warning. Remaining plan parts split into #stuckcycleorphan, #stuckcyclerepair, and #stuckcyclesuperseded. Plan: tasks/agent-doc/plan-stuck-cycle-causes-duplicated-uncommitted-response.md.
- [/] [#jbresstart] Refuse cross-file pane rescue and extend Starting-actor wait after stash recovery; see tasks/agent-doc/plan-jb-route-rescued-stash-pane-starting-actor.md
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
