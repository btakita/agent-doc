---
agent_doc_session: tsift-v0.1
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review tsift codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. '#follow-up-backlog'
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: start
---

## Status

<!-- agent:status patch=replace -->

complete
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260611-151636.md`*

Compacted content:
- Archived 1 response topic(s): #next-steps to fix tsift bugs
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...

### Re: do #logsigfp — anchor npm/pnpm error classification — opus-4-8

> **Queue prompt:**
>
> do [#logsigfp]


Done. Fixed the false-positive risk the `#loggate` classifier introduced.

**Root cause:** `classify_generic_signal` matched `err!` and `err_pnpm` as bare substrings, so benign lines were misflagged as errors — `stderr!` contains `err!`, and any token/URL containing `err_pnpm` (e.g. a docs path) matched too.

**Fix** (`packages/tsift-digest/src/log_digest.rs`): a new `has_npm_pnpm_error_token` helper anchors detection to whitespace tokens — an `ERR!` token (npm/yarn) or a token prefixed `ERR_PNPM_` (pnpm). I tightened the pnpm prefix to the full `ERR_PNPM_` (with trailing underscore) after the regression test caught that `err_pnpmish` slipped through an 8-char `err_pnpm` prefix.

**Live proof** (installed binary):

```
input:  "flushed stderr! buffer"  +  "npm ERR! code ELIFECYCLE"
signals: error x1 — "npm ERR! code ELIFECYCLE"   (stderr! correctly NOT flagged)
```

Proof — changed paths: `log_digest.rs`, `VERSIONS.md`. Commands: `make check` (exit 0), `cargo install` (ok), live smoke, `git push` (`64f30cf..32ae62c`). Verification: 9/9 `tsift-digest` log_digest tests (incl. new `npm_pnpm_error_classification_is_token_anchored`) + the `#loggate` fixture-gate integration test still pass (npm/pnpm cases still classify their real markers). CI run 27357574146 pending. Confidence: high.

Marked `#logsigfp` done. Backlog now: `#logfixturecov` (top — add forbidden_signals cases to lock this in), then `#tsinflight`.

### Re: do #logfixturecov — false-positive guard for the fixture gate — opus-4-8

> **Queue prompt:**
>
> do [#logfixturecov]


Done. Hardened the `#loggate` fixture gate so it now proves the classifier doesn't *over*-flag, not just that it doesn't *under*-flag.

**Key refinement:** `forbidden_signals` now checks the **classified signal messages** (`signal_message_text`), not the full digest projection. Building the precision case surfaced why this matters — a benign line's tokens can still appear in the digest via other channels even when the line is correctly *not* a signal:
- A symbol-like token (`err_pnpmish-fixture`, underscore → symbol-extracted) lands in `symbol_refs`.
- An anchored path lands in `file_refs`.

So a naive full-projection forbidden check would false-fail. Checking classification output only is the precise "this benign line was not flagged as an error" semantic.

**Added:**
- `classifier-false-positive-precision` fixture case (npm) — benign `stderr!`, `docs/err_pnpm-notes.md`, `err_pnpmish-fixture` lines must NOT classify, while real `npm ERR!` / `ERR_PNPM_LIFECYCLE_FAIL` still do. 62.7% savings.
- Unit test proving the symbol-leak token is in `digest_signal_text` but absent from `signal_message_text`.
- Gate-integration assertion that no case has `present_forbidden_signals`.

Proof — changed paths: `log_digest.rs`, `tests/exit_code.rs`, `fixtures/log-digest-token-savings.json`, `specs/digests-sessions.md`, `VERSIONS.md`. Commands: `make check` (exit 0), `cargo install` (ok), `git push` (`32ae62c..35f9c35`). Verification: 10/10 `tsift-digest` log_digest tests + gate integration test (now 6 cases, all 5 ecosystems, zero forbidden hits) pass; full suite green. CI pending. Confidence: high.

Marked `#logfixturecov` done. Last queue item: `do [#tsinflight]` (resolve the 16 in-flight uncommitted files) — note that one is unrelated WIP I didn't author, so I'll assess carefully rather than presume to land it.
<!-- agent:boundary:7765ab45 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
- ~~do [#logsigfp]~~
- ~~do [#logfixturecov]~~
- do [#tsinflight]
<!-- /agent:queue -->
<!--
https://github.com/chopratejas/headroom
-->

## Backlog

<!-- agent:backlog priority -->
- [ ] [#tsinflight] [recommended] Resolve the 16 in-flight uncommitted tsift working-tree changes (graph-db/surrealdb/token-gate/finding/health). session-check flags them as partial-staging risk and they block clean committed-tree verification; land or revert them in their own cycle.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
<!-- /agent:review -->

<!-- agent:icebox -->
- [ ] [#g5kw] What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
- [ ] [#4z0w] Can we increase max-hop caps?
- [ ] [#cg52] Could FalkorDB help with performance?
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
