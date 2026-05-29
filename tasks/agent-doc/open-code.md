---
agent_doc_session: 7910a62f-ef5f-4f02-9703-dade1f077521
agent: claude
claude_model: opus
opencode_model: GLM-5.1
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix.
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#next-steps': Any follow-up items to place in the acklog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue_active: false
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/fb8de777e65629bf156ef66517a44d0d9c829c7e89b02b868f8ca9d0519d3ddc-20260529-015549.md`*

Compacted content:
- Archived 2 response topic(s): queue tidy → #bz6s Wave 5 complete (extraction done); do [#yybc]
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/fb8de777...
❯ What are #next-steps to split out modules?
### Re: #next-steps to split out modules — opus-4-7

First, the honest constraint: the big extraction is done and `agent-doc-orchestration` is **deliberately one crate** (Direction A). The internal cycles that forced that — `git↔cycle_state`, `capture↔repair`, `route↔sync` — still block cleanly splitting the *cluster* into more crates. So "split out more modules" has real limits; the feasible moves are narrower than the original extraction.

Also closed **`#adcr`** (extract `agent-doc-core`) — it was already satisfied (core is extracted + published 0.1.0); the binary auto-struck its queue item last cycle, now marked done in the backlog too.

**Top of backlog: `#adoc-pure-to-core`.**

#### Next steps, by priority (added to backlog)

| # | Item | Value / risk |
|---|------|--------------|
| 1 | **`#adoc-pure-to-core`** | Push genuinely-pure modules from orchestration down to the fast-building `agent-doc-core`. Confirmed-clean now: **`heuristics`** (zero deps) and **`replay_guard`** (only the core-backed `template` shim). Others (`dedupe`/`callback`/`editor_route_errors`) are blocked by `snapshot → git` I/O. **Clean, low-risk, low-but-real value** — shrinks the 340-dep crate. |
| 2 | **`#adoc-sqlite-isolation`** `[recommended]` | The single biggest build cost in orchestration is **`rusqlite` (bundled SQLite — a large C compile)**. It's used by `archive_index` + `project_controller`. A feasibility spike: can `archive_index` be carved into its own crate to isolate that C build from orchestration recompiles? **Blocker:** `project_controller` (cluster-central) also uses rusqlite, so it likely needs an interface seam (inject a DB handle/trait) first. **High value, uncertain feasibility — spike before committing.** |
| 3 | **`#adoc-orch-shim-cleanup`** | Rewrite CLI-shell `crate::X` → `agent_doc_orchestration::X` and delete the `pub use` shims in `agent-doc/src/{lib,main}.rs`. **Mechanical polish**, finishes the migration cleanly — not a true split, lowest priority. |

#### What's *not* worth attempting

Splitting the tmux/PTY/route/sync/git cluster into separate crates — the cycle entanglement that drove Direction A still applies; you'd be back to breaking `route↔sync`/`git↔cycle_state`. The build-time win there is better pursued via the `#adoc-sqlite-isolation` seam (isolate the heaviest *dep*, not the entangled modules).

Want me to start with `#adoc-pure-to-core` (the clean one) or run the `#adoc-sqlite-isolation` feasibility spike first?

### Re: do [#adoc-pure-to-core] — heuristics + replay_guard relocated to core — opus-4-7

You queued `do [#adoc-pure-to-core]` at the head (answering last cycle's question — start with the clean one), so I did it.

#### What landed

- **`heuristics` → `agent-doc-core`** — pure recommendation-detection logic, zero crate/external deps. Trivially core-able.
- **`replay_guard` → `agent-doc-core`** — uses only `template::parse_patches`, which lives in `agent_doc_core::template`, so `crate::template::parse_patches` resolves intra-core after the move (no rewrite).
- `agent-doc-orchestration` re-exports both via `pub use agent_doc_core::{heuristics, replay_guard}`, so its call sites (`write`, `session_check`, `codex_hook`, `repair`) compile unchanged. Their tests moved with them — **core suite 620 → 645**.

#### Verification

- `make check` green (clippy + full suite) modulo the pre-existing live-tmux `codex_bare_run` test. Core: 645 passed.
- `cargo build --release` green; `nm -D libagent_doc.so` — full 39-symbol FFI surface intact.
- Commits: submodule `e6c25016`, superproject pointer `08faa1e3b`. `#adoc-pure-to-core` marked done.

This shrinks the 340-dep orchestration crate by two pure modules (moving them to the fast-building leaf). It's a small, clean win — the audit confirmed the other candidates (`dedupe`/`callback`/`editor_route_errors`) are blocked by `snapshot → git` I/O and can't follow.

#### Queue remaining

`do [#adoc-sqlite-isolation]` `[recommended]` (feasibility spike on isolating rusqlite/bundled-SQLite — the high-value, uncertain-feasibility one) and `do [#adoc-orch-shim-cleanup]` (mechanical polish). Say the word for the sqlite spike, or I can take the shim cleanup if you'd rather bank a safe one.

### Re: do [#adoc-pure-to-core] — heuristics + replay_guard relocated to core — opus-4-7

You queued `do [#adoc-pure-to-core]` at the head (answering last cycle's question — start with the clean one), so I did it. (Also fixed the queue tag: `&lt;!-- agent:queue done --&gt;` had a malformed attribute — `done` isn't a valid queue mode, only `auto` is — so I reset it to the default `&lt;!-- agent:queue --&gt;`.)

#### What landed

- **`heuristics` → `agent-doc-core`** — pure recommendation-detection logic, zero crate/external deps.
- **`replay_guard` → `agent-doc-core`** — uses only `template::parse_patches`, which lives in `agent_doc_core::template`, so `crate::template::parse_patches` resolves intra-core after the move (no rewrite).
- `agent-doc-orchestration` re-exports both via `pub use agent_doc_core::{heuristics, replay_guard}`, so its call sites (`write`, `session_check`, `codex_hook`, `repair`) compile unchanged. Their tests moved with them — **core suite 620 → 645**.

#### Verification

- `make check` green (clippy + full suite) modulo the pre-existing live-tmux `codex_bare_run` test. Core: 645 passed.
- `cargo build --release` green; `nm -D libagent_doc.so` — full 39-symbol FFI surface intact.
- Commits: submodule `e6c25016`, superproject pointer `08faa1e3b`. `#adoc-pure-to-core` marked done.

Shrinks the 340-dep orchestration crate by two pure modules. The audit confirmed the other candidates (`dedupe`/`callback`/`editor_route_errors`) are blocked by `snapshot → git` I/O and can't follow.

#### Queue remaining

`do [#adoc-sqlite-isolation]` `[recommended]` (the high-value, uncertain-feasibility rusqlite-isolation spike) and `do [#adoc-orch-shim-cleanup]` (mechanical polish). Say the word for the sqlite spike, or I can bank the safe shim cleanup.
<!-- agent:boundary:3ab9af25 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue -->
preset #spec-test-build-install-commit-push
- do [#adoc-pure-to-core]
- do [#adoc-sqlite-isolation]
- do [#adoc-orch-shim-cleanup]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#adoc-sqlite-isolation] [recommended] Feasibility spike: carve archive_index (+ rusqlite/bundled-SQLite, the slowest build dep) into its own crate to isolate the C build from agent-doc-orchestration recompiles. Blocker to investigate first: project_controller ALSO uses rusqlite and is cluster-central, so this likely needs an interface seam (pass a DB handle/trait in) before the split is clean. Spike before committing.
- [ ] [#adoc-orch-shim-cleanup] Finish the migration: rewrite CLI-shell crate::X call sites to agent_doc_orchestration::X and delete the pub use shims in agent-doc/src/{lib,main}.rs. Mechanical/low-risk polish, not a true crate split. Plan's optional final step.
<!-- /agent:backlog -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc/open-code.done.md -->

<!-- completed work archived in tasks/agent-doc/open-code.done.md -->
<!-- /agent:done -->
