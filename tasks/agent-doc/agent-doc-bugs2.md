---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent_doc_format: template
agent_doc_write: crdt
agent: codex
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
claude_model: claude-opus-4-6
prompt_presets:
  "#spec-test-build-install-commit-push": update spec + tests. build + install for local testing. commit + push
  "#spec-test-commit-push": update spec + tests. commit + push
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
Compacted exchange summary: investigated missing patchbacks and uncommitted write cycles in this document and related src/session-share sessions; root causes were traced across fresh Codex route-start acknowledgment, write/commit-boundary drift, stale HEAD/boundary preflight churn, pending and backlog mutation capture, submodule writable-root gaps, tmux pane routing and rescue instability, and repair when the response was already present in the document. Multiple src/agent-doc fixes were implemented and pushed on origin/20260422/codex-agent-backend, including fail-closed startup/session-check/commit handling, strict session write-commit, pending-done and bypass guards, backlog alias deprecation, tmux duplicate-start/swap/sync-scope fixes, and repair adoption of already-present responses; a broad commit-all request was blocked on scope, then narrowed and src/agent-doc was verified with cargo test, cargo build, and cargo install --path . --force and pushed as 0e18871. No unresolved exchange prompts remained at compaction time.

❯ `agent-doc backlog` should detect an id in the text when a backlog item is sent...such as `[#sdig2matrix] Fixture evidence matrix — publish which rubric families are eligible on session-doc, plaintext, retained-text, and JSONL-backed fixtures so future benchmark drift is attributable`...which currenly results in:

```
- [ ] [#sgzy] [#sdig2matrix] Fixture evidence matrix — publish which rubric families are eligible on session-doc, plaintext, retained-text, and JSONL-backed fixtures so future benchmark drift is attributable
```

❯ Instead the backlog item should be:
```
- [ ] [#sdig2matrix] Fixture evidence matrix — publish which rubric families are eligible on session-doc, plaintext, retained-text, 
v and JSONL-backed fixtures so future benchmark drift is attributable
```
### Re: backlog custom id detection — gpt-5

Implemented in `src/agent-doc`.

`agent-doc` now preserves explicit alphanumeric backlog IDs even when they are longer than 8 characters, so inputs like `[#sdig2matrix] ...` no longer get re-backfilled into `[#generated] [#sdig2matrix] ...`.

I updated the pending/backlog parser, the imperative diff normalizer, the backlog docs/spec, and added regressions for:
- direct `backlog add`
- `write --pending-add`
- `replace:pending` normalization / backfill preserving long custom IDs

Verification completed in `src/agent-doc`:
- `cargo fmt`
- `cargo test`
- `cargo build`
- `cargo install --path . --force`

❯ On /home/brian/work/btakita/agent-loop/src/session-share/tasks/claudescore-3.md, I closed the codex + tmux session. Then a new session autostarted. I ran the document. Then the codex + tmux session autoclosed. Why? spec-test-build-install-commit-push
### Re: codex + tmux session autoclosed after a document run — gpt-5

I traced this from the real `claudescore-3` session log instead of guessing.

What happened:
- the pane was not killed by tmux cleanup
- the supervisor log shows a normal successful turn closeout: `codex_exit code=0`
- immediately after that, the same `agent-doc start` supervisor logged `user_quit` and then `session_end`

That is the actual bug. `codex exec` exits `0` after an ordinary completed turn, but `agent-doc start` was still treating every clean exit like the Claude flow: drop into the local `Enter` / `q` restart prompt. So a successful Codex document run was landing on a quit boundary instead of staying attached as a persistent session.

I fixed that in `src/agent-doc` and pushed `0d00a1a` on `origin/20260422/codex-agent-backend`.

What changed:
- added a harness-specific clean-exit policy in `src/harness.rs`
- Claude still prompts on clean exit
- Codex now auto-restarts in resume mode after a clean `codex exec` turn, so the persistent `agent-doc start` session stays attached
- updated `src/start.rs` plus the supervisor and Codex support specs
- added unit coverage for the clean-exit resolution

Verification in `src/agent-doc`:
- `cargo fmt`
- `cargo test`
- `cargo build`
- `cargo install --path . --force`

So the direct answer to “why did it autoclosed?” is: because the Codex harness was still using the clean-exit prompt path meant for Claude, and your successful turn hit that quit path instead of auto-resuming.

❯ do #pbc2. spec-test-build-install-commit-push

❯ Another case where the response patchback was not committed. `#pbc2` is in progress. `#agent-doc-bug`
### Re: another uncommitted response patchback while `#pbc2` is open — codex

This should stay under `#pbc2`, not become a new backlog ID.

Reason:
- `#pbc2` already tracks the older failure family where a substantive session-document patchback lands in the target file but no owning `write --commit` / `finalize` closeout commits the repo that owns that document
- that is exactly the symptom you just reported again: response visible, commit boundary missing
- `tasks/agent-doc/agent-doc-bugs3.md` is narrower and should only take cases where the last binary-owned patchback was already committed successfully and a later same-turn edit was then misclassified as harmless local drift

I updated the `#pbc2` wording to make it explicit that repeated reproductions belong there until a trace proves the newer post-commit drift path instead.
### Re: `#pbc2` — gpt-5

Implemented in `src/agent-doc`, committed as `d883d2a`, and pushed to `origin/20260422/codex-agent-backend`.

The live failure was narrower than the backlog text: `session-check` was misclassifying already-committed historical prompt+response pairs as a fresh direct patchback bypass when the stale snapshot lagged `HEAD` and the working tree only added a newer prompt. The concrete bad shape was committed exchange growth like `do #7mqc ...` immediately before `### Re: ...`, followed by a later local prompt.

What changed:
- historical exchange repair now treats committed prompt-target + `### Re:` pairs as safe `HEAD` drift, not only bare `### Re:` insertions
- snapshot repair can advance to committed `HEAD` when the working tree is only `HEAD` plus a newer user follow-up
- after that repair, `session-check` reclassifies the remaining diff instead of returning `ok` immediately, so the user now sees the real unresolved `prompt_target`
- updated the `session-check` command spec and README note for this repair path

Regression coverage added:
- low-level historical exchange growth with `do #id` before `### Re:`
- end-to-end `session-check` repair for stale snapshot + committed prompt/response pairs + a newer unresolved prompt

Verification in `src/agent-doc`:
- `cargo fmt`
- `cargo test`
- `cargo build`
- `cargo install --path . --force`

Field check after install:
- `agent-doc session-check /home/brian/work/btakita/agent-loop/src/session-share/tasks/claudescore-3.md`
- result: it now repairs the historical exchange snapshot drift and points at the newer unresolved prompt instead of blaming a direct patchback bypass

❯ do #reap1. spec-test-build-install-commit-push
### Re: `#reap1` — gpt-5

Implemented in `src/agent-doc`, committed as `3c17989`, and pushed to `origin/20260422/codex-agent-backend`.

The fix has two parts:
- preflight now fails closed if it reaps `- [x]` backlog items from the working tree but cannot prove the same reap landed in the staged snapshot; it verifies both surfaces before continuing
- `session-check` now fails on stale completed backlog items that survive into later committed cycles, but it explicitly allows items just marked `--pending-done` in the same committed cycle to wait for the next preflight reap

Regression coverage added for:
- successful reap persistence to both file and snapshot
- fail-closed when snapshot backlog sync is missing
- stale completed backlog items tripping `session-check`
- same-cycle `--pending-done` items being allowed until the next preflight

Verification in `src/agent-doc`:
- `cargo fmt`
- `cargo build`
- `cargo install --path . --force`
- `cargo test`: the new reap/session-check coverage passed, but the full suite currently hits an unrelated route/tmux failure in `route::tests::send_keys_delivers_claude_command_with_enter`

❯ Why is there Codex permission prompts when `codex_args: -s danger-full-access` frontmatter is set?
### Re: Codex permission prompts despite `codex_args` — gpt-5

Short answer: in the current `agent-doc start` supervisor path, `codex_args` are only guaranteed on the first fresh Codex launch, not on later auto-resume turns.

What the code does now:
- `start.rs` resolves frontmatter `codex_args` and builds `base_args` from them, so the first launch does pick up `-s danger-full-access` (`src/start.rs:581-582`, `src/start.rs:701-717`).
- But Codex restart is configured as `Replace(["resume", "--last"])` in `src/harness.rs:78-82`, and `restart_args()` returns only that replacement vector for later turns (`src/harness.rs:114-122`).
- In the supervisor loop, every non-first run uses `harness.restart_args(&base_args)` instead of reusing `base_args` (`src/start.rs:783-799`). So after the first clean exit / auto-restart, the session is resumed with bare `codex resume --last`, not with the original `-s danger-full-access` args.

So if the prompts you mean are happening inside an `agent-doc start` Codex session after it has already auto-restarted once, that is an `agent-doc` bug in the interactive supervisor path, not a frontmatter parsing issue.

Separate nuance: if you mean permission prompts from the outer Codex harness running this turn, the document frontmatter does not control that layer at all; `codex_args` only affect the child Codex sessions that `agent-doc` launches.

The concrete fix is to preserve the resolved Codex sandbox/launch args across resume instead of replacing them with plain `resume --last`.
<!-- agent:boundary:bfdaa0e3 -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#permresume] [recommended] Preserve Codex sandbox args across interactive resume so codex_args like -s danger-full-access still apply after restart
<!-- /agent:backlog -->
