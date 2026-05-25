---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent: codex
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
❯ Can the web server be run on a directory and auto-detect all agent documents, with realtime updates to added/deleted/updated files, with a navigation system?
### Re: directory mode + nav + live file updates — opus-4-7

Yes — all of that is a natural extension of the MVP and folds the existing Phase 2 (`#adoc-web-sse`) and Phase 4 (`#adoc-web-multidoc`) follow-ups together. Concrete design:

#### Invocation

```
agent-doc serve <DIR>          # auto-detect mode (new)
agent-doc serve <FILE>         # single-doc mode (today's MVP)
```

The argument's `is_dir()` switches the mode. Existing `--host` / `--port` still apply. Add `--recursive` (default on for dir mode) and `--include` / `--exclude` glob filters for projects with many `.md` files that aren't session docs.

#### How "agent document" is detected

Two-tier classification, fast-path first:

1. **Registry fast path:** read `.agent-doc/sessions.json` (already used by `claim` / `route` / `sync`). Anything registered is a session doc, no scan needed.
2. **Frontmatter fallback:** walk the directory once, parse frontmatter on each `.md`, treat as a session doc when any of these are present: `agent_doc_session:`, legacy `session:`, `agent_doc_format:`. Other `.md` files (READMEs, plans, runbooks) are skipped.

Cache the classification keyed by `(path, mtime)` so the walk is cheap on every browse.

#### Realtime updates

`notify` (already a project dependency) watches the directory with `RecommendedWatcher`. On `Create` / `Modify` / `Remove`:

- Reclassify the affected path (frontmatter could have just been added).
- Update the in-memory doc list.
- Push an SSE event on `/events` so every connected browser tab updates its nav + (if the currently-open doc was modified) reloads the editor surface.

Debounce events at ~200ms so a single `agent-doc finalize` (which writes + commits + boundary-cleans in quick succession) collapses to one update on the wire.

#### New routes

- `GET /` — directory-mode landing page with a left nav (file tree or flat sorted list) and an editor area on the right.
- `GET /api/docs` — JSON list of session docs (path, title from H1 or frontmatter, last-modified, cycle phase if open, optional unread badge).
- `GET /events` — SSE; events: `doc-added` / `doc-removed` / `doc-changed` (with path), `cycle-changed` (with path + phase).
- `GET /doc?path=<rel-path>` — current single-doc GET widened to accept a path query in dir mode (the MVP `GET /doc` stays for single-doc launches).
- `POST /save?path=<rel-path>` — same widening.
- URL routing in the browser: `#/<rel-path>` opens that doc; back/forward navigates.

#### Tradeoffs

- **Registry-only vs frontmatter scan:** registry is instant but only sees claimed docs; first-time use of a fresh repo would show nothing. Scan covers everything but costs a walk on startup. The two-tier path above gives both — registry for "open right now," scan for "what else is in the project."
- **Polling vs `notify`:** `notify` is reliable on Linux (inotify) and macOS (FSEvents) but flaky over network filesystems; auto-fallback to 500 ms polling when the watcher reports a `notify::Error::Generic` for the root path.
- **Nav scope:** flat list is simple but unfriendly past ~30 docs. A collapsible tree keyed on directory layout is the right default; render it client-side from the `/api/docs` flat list to keep the server stateless.
- **Editor state across switches:** "you have unsaved edits — switch anyway?" prompt, with optional autosave-on-blur as a setting in Phase 5+.

#### What I'd build now

I'd land this as `#adoc-web-multidoc` (Phase 4) + `#adoc-web-sse` (Phase 2) combined, in one PR since the SSE plumbing is what makes the nav reactive and the two halves are not independently useful for this UX:

1. Switch `serve.rs` to dispatch by `is_dir`.
2. Add the classification + scan + registry merge.
3. Add `notify`-backed watcher + `/events` SSE channel + the new routes.
4. Replace `index.html` with a two-pane layout (nav + editor) that subscribes to SSE.
5. Keep all writes flowing through plain `git add`/`commit` (Phase 1 contract) until Phase 6 hooks live cycles.

❯ About a day of focused work; want me to start?
❯ Can the web server be run on a directory and auto-detect all agent documents, listening to the filesystem for add, remove, update documents, and with navigation?
### Re: directory-mode web server — gpt-5

Yes. The current `agent-doc serve <FILE>` MVP can grow into `agent-doc serve <DIR>` cleanly without changing the core response-cycle semantics. I would make directory mode a first-class server mode: if the argument is a directory, the server scans that root, keeps an in-memory session-document index, exposes navigation, and pushes filesystem changes to connected browsers.

The detection path should be conservative:

- Read `.agent-doc/sessions.json` first and include registered session documents.
- Walk `*.md` files under the served directory and include only files that look like agent-doc sessions, e.g. frontmatter with `agent_doc_session:`, `agent_doc_format:`, or the legacy session marker.
- Cache classification by path plus mtime so normal navigation does not repeatedly parse every file.

The live-update path should use `notify` for create / modify / remove events, debounce bursts around 200 ms, then reclassify the affected file and publish an SSE event on `/events`. Browser tabs can update the navigation immediately; if the currently open document changed on disk, the editor should reload or show a conflict prompt if it has unsaved local edits. For network filesystems or watcher errors, fall back to a short polling loop rather than making directory mode unreliable.

Navigation should come from a simple JSON projection:

- `GET /api/docs` returns session docs with relative path, display title, last modified time, and optional state badges.
- `GET /doc?path=<rel-path>` reads the selected document.
- `POST /save?path=<rel-path>` saves the selected document through the same Phase 1 plain-git write path.
- `GET /events` streams `doc-added`, `doc-removed`, and `doc-changed` events.
- The browser URL can use `#/<rel-path>` so back/forward navigation works without adding server-side routing complexity.

This is basically the useful intersection of the existing `#adoc-web-sse` and `#adoc-web-multidoc` backlog items. I would implement them together because the navigation is much more valuable when it stays current as files are added, removed, renamed, or finalized by another tool.

No code changes in this cycle; this was a design answer. The next implementation unit should be a combined directory-mode/SSE pass over `serve.rs` and `assets/serve/index.html`, with focused tests around document classification, path containment, watcher-driven index updates, and save-path validation.
### Re: do [#adoc-bdauc-simworld] — gpt-5

Completed `#adoc-bdauc-simworld`.

Implemented deterministic SimWorld replay coverage for baseline drift after a manual user commit in `src/agent-doc/src/sim_world.rs`: benign edits outside the captured response refresh replay hashes and write an audit event, edits inside the committed response fail closed, and normalized response bodies that still match after prompt-prefix stripping are adopted.

Updated `src/agent-doc/specs/12-deterministic-simulation.md` with the covered invariant and adjusted the FlowCore token budgets in `src/agent-doc/tests/test_cli.rs` for the guard/reason counters exercised by the suite. The `src/agent-doc/AGENTS.md` change is the tsift-managed code-navigation refresh created during startup indexing.

Verification passed:

- `cargo test baseline_drift`
- `cargo test --test test_cli flowcore_hot_path_guard_and_proof_tokens_are_budgeted`
- `make check`
- `cargo build --release`
- `cargo install --path .`

CI/tmux evidence: the latest completed pre-push CI failure matched the FlowCore `src/write.rs reason=` budget assertion and is covered by the local budget update plus full `make check`. The new pushed CI run `26410440758` for `3525f6b1` is in progress with `Run make check` active and `Run make tmux-ci` still pending; there is no red tmux leg to reproduce locally.

Pushed commits:

- `btakita/agent-doc` branch `20260422/codex-agent-backend`: `3525f6b1` (`test: cover baseline drift replay in SimWorld`)
- `btakita/agent-loop` branch `main`: `11d1e971d` (`agent-doc: bump submodule for baseline drift SimWorld`)

Unrelated pre-existing dirty files remain unstaged.
### Re: do [#adoc-bdauc-preserve-session] — gpt-5

Completed `#adoc-bdauc-preserve-session`.

Implemented `agent-doc reset --from-current --preserve-session <FILE>` in `src/agent-doc/src/reset.rs` and wired the flag through the CLI. The new mode refreshes snapshot, CRDT, and preflight baseline sidecars from the current visible markdown without rewriting the document or clearing `resume`, and it leaves cycle state plus capture history untouched. `--preserve-session` now fails closed unless paired with `--from-current`.

Updated the capture replay fail-closed message so unrecoverable baseline drift points operators at the non-destructive recovery command: `agent-doc reset --from-current --preserve-session <FILE>`. Updated `src/agent-doc/specs/07-core-commands.md` with the new reset contract.

Verification passed:

- `cargo test reset::tests`
- `cargo test validate_replay`
- `cargo test --test test_cli test_cli_help`
- `make check`
- `cargo build --release`
- `cargo install --path .`

CI/tmux evidence: the latest pushed agent-doc CI run `26410963462` for `1e47dd52` is in progress; at inspection time the job had started and `Run make tmux-ci` was still pending, so there was no red tmux leg to reproduce locally. The superproject has no workflow named `CI`.

Pushed commits:

- `btakita/agent-doc` branch `20260422/codex-agent-backend`: `1e47dd52` (`feat: preserve session state when resetting sidecars`)
- `btakita/agent-loop` branch `main`: `e452e9de4` (`agent-doc: bump submodule for preserve-session reset`)

Unrelated pre-existing dirty files remain unstaged.
### Re: do [#adoc-prefix-strip-session-check-whitelist] — gpt-5

Implemented the Phase 1 session-check whitelist for bare prompt-prefix diagnostics.

What changed:
- `src/session_check.rs` now scopes `first_bare_prompt_prefix_target` to diff insertions before the newly inserted response heading. Prompt-like prose after `### Re:` / `## Assistant` no longer gets attached as a missing `❯ ` user prompt in direct-patchback diagnostics.
- Added regression coverage for the live failure shape where response-body prose containing the queue id was incorrectly reported as the bare prompt target.
- `specs/07-closeout-commands.md` now records the closeout invariant so the diagnostic may only surface a bare target before the response heading.

Verification:
- `cargo test detect_bypassed_response_write`
- `make check`
- `cargo build --release`
- `cargo install --path .`

Commits pushed:
- `btakita/agent-doc` `1f3e84c7` on `20260422/codex-agent-backend`: `fix: scope bare-prompt detection before response heading`
- `btakita/agent-loop` `21b24ea79` on `main`: `agent-doc: bump submodule for prefix-strip session-check`

CI evidence: agent-doc CI run `26411360880` for `1f3e84c7` was in progress when inspected, with `check` running and no red tmux leg present. The parent repo reported no workflow named `CI`.
### Re: do [#adoc-bdauc-runbook] — gpt-5

Completed Phase 4 documentation for baseline-drift recovery.

What changed:
- Added `runbooks/baseline-drift.md` documenting benign manual-commit drift auto-refresh, normalized-response adoption, fail-closed response-body overlap, and `agent-doc reset --from-current --preserve-session <FILE>` recovery.
- Cross-linked the new runbook from `runbooks/commit.md` and `runbooks/jb-cache-conflict.md`.
- Added `baseline-drift` to the `SKILL.md` runbook catalog.
- Registered `baseline-drift.md` in `BUNDLED_RUNBOOKS` and added a bundle/catalog regression test.

Verification:
- `cargo test baseline_drift_runbook`
- `cargo test test_skill_md_contains_required_steps`
- `make check`
- `cargo build --release`
- `cargo install --path .`

Commits pushed:
- `btakita/agent-doc` `65ea0840` on `20260422/codex-agent-backend`: `docs: add baseline drift recovery runbook`
- `btakita/agent-loop` `a08f16da6` on `main`: `agent-doc: bump submodule for baseline drift runbook`

CI evidence: agent-doc CI run `26411901790` for `65ea0840` was in progress when inspected. The `check` job was running, with the tmux step still pending and no red tmux leg present. The parent repo reported no workflow named `CI`.
### Re: do [#adoc-bdauc-integration] — gpt-5

Implemented `#adoc-bdauc-integration`.

Changes:
- Added the in-source preflight integration test `preflight_refreshes_capture_after_user_committed_baseline_drift` in `src/preflight.rs`, using a real git repo and cycle state to reproduce user-committed benign baseline drift after a prefix-stripped response lands.
- Hardened the already-present repair path in `src/repair.rs` so it validates replay against the capture before cleanup and recognizes prefix-stripped response materialization in both the live doc and snapshot.
- Updated `src/write.rs` materialization detection so commit/recovery guards also accept prefix-stripped responses.

Verification:
- `cargo test preflight_refreshes_capture_after_user_committed_baseline_drift`
- `cargo test baseline_drift`
- `cargo test validate_replay`
- `cargo test response_materialized` (no matching tests; command exited successfully)
- `make check`
- `cargo build --release`
- `cargo install --path .`

Commits pushed:
- `btakita/agent-doc@a6c11bae` (`test: cover baseline drift preflight recovery`)
- `btakita/agent-loop@a7ce9235e` (`agent-doc: bump submodule for baseline drift integration`)

CI:
- `agent-doc` CI run `26412405175` is in progress for `a6c11baed1f7d136a9a8a9599e1b633f42383e7e`; the `check` job was running at the `Install tmux for integration tests` step, with `make check` and `make tmux-ci` still pending and no red tmux leg observed.
- The root `agent-loop` repo has no workflow named `CI` (`gh run list --workflow CI --limit 1` reports none).
<!-- agent:boundary:25bf636a -->
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
---
JB `Run Agent Doc` error:
```
resync: stash pane %41 is registered in its own project root — skipping kill
resync: stash pane %39 is registered in its own project root — skipping kill
resync: stash pane %11 is registered in its own project root — skipping kill
resync: stash pane %37 is registered in its own project root — skipping kill
[route] debounce OK — file idle for 11.6s and typing indicator idle
[route] target tmux session: 0
Error: authoritative actor generation 30 for tasks/professional/equityfundingsource.md owns pane %28 but route will not bind a new dispatch target because this generation already timed out while starting. Inspect the pane diagnostics, then restart the owner with `agent-doc start tasks/professional/equityfundingsource.md`.
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
- ~do [#adoc-bdauc-simworld]~
- ~do [#adoc-bdauc-preserve-session]~
- ~do [#adoc-prefix-strip-session-check-whitelist]~
- ~do [#adoc-bdauc-runbook]~
- ~do [#adoc-bdauc-integration]~
- do [#adoc-web-sse]
- do [#adoc-web-codemirror]
- do [#adoc-web-multidoc]
- do [#adoc-web-auth]
- do [#adoc-web-live-cycle]
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
