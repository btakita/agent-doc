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
queue_active: false
---

## Status

<!-- agent:status patch=replace -->

Reviewed next steps; top backlog item: #autodag.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260522-020859.md`*

Compacted content:
- Archived 1 response topic(s): auto-DAG queue work
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
❯ Investigate the logs for agent-doc bugs. Particularly with creating a new document then running JB `Run Agent Doc`.

❯ Not that as I was typing this prompt, the duplicate prompt was inserted. Please fix in a mathematically provable way.

❯ #spec-test-build-install-commit-push
### Re: route typing quiescence — gpt-5

I found the live-typing failure at the route boundary. The JetBrains action waited in-process, but the binary route path still defaulted to `--debounce 0`, so `agent-doc route --dispatch-only --plain-trigger` could proceed into route-side document mutation and pane input without a cross-process typing proof. The ops history matched that shape: route dispatched while duplicate-prompt repair and preflight cleanup were still seeing live prompt residue.

The fix makes the route invariant binary-owned. `agent-doc route` now defaults to a 500ms quiescence gate and, before it inserts a missing session id, scrubs duplicate prompt comments, or submits a reopen, it requires both the file mtime and `.agent-doc/typing` editor indicator to be idle for the debounce window. If either signal stays active through the bounded wait, route fails closed with `typing_active` and `mtime_idle_ms` in the diagnostic instead of proceeding. Under the existing editor contract that every buffer edit publishes `document_changed(file)`, the mutation/dispatch precondition is now explicit: route only crosses the visible side-effect boundary after the last recorded edit signal is at least the debounce window old.

Changed paths in `src/agent-doc`: `src/main.rs`, `src/route.rs`, `src/route/tests.rs`, `SPEC.md`, `README.md`, `specs/11-debounce.md`, `specs/07-session-tmux-commands.md`, and `editors/jetbrains/SPEC.md`.

Verification completed:

- `cargo test route_debounce -- --test-threads=1` passed.
- `cargo test --test test_cli test_cli_route_file_not_found -- --exact` passed after preserving the stable missing-file diagnostic.
- `make check` passed on the final rerun: clippy, full Rust test suite, medium SimWorld corpus, and version sync.
- `make build-release` passed.
- `make install` passed; `agent-doc --version` reports `agent-doc 0.33.16`.
- Latest GitHub CI run `26263814658` for `20260422/codex-agent-backend` was successful; its `Run make check` and `Run make tmux-ci` steps both completed successfully on 2026-05-22.

Commits:

- `src/agent-doc` commit `b0b9a83f` (`fix: gate route dispatch on typing quiescence`) is pushed to `origin/20260422/codex-agent-backend`.
- Superproject pointer commit `f47752cc0` (`agent-doc: update route quiescence pointer`) is committed. The root push runs after this binary closeout so the session document stays on the finalize-owned path.
<!-- agent:boundary:d96f2dde -->
<!-- /agent:exchange -->
###
<!--
File Cache Conflict
-->

<!-- agent:queue -->
<!-- /agent:queue -->

## Pending / Not Built

<!-- agent:backlog -->
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
