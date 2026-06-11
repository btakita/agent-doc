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
queue: stop
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260611-151219.md`*

Compacted content:
- Archived 6 response topic(s): queue closeout recovery; log compression; #logfam log compression P1 (semantic line families); 3 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...

### Re: #next-steps to fix tsift bugs — opus-4-8

> **Queue prompt:**
>
> What are #next-steps to fix tsift bugs?


First, the good news: the `#loggate` CI run (27324835879) is **green**, so all three shipped log-compression items are verified on CI's committed tree. The one red run was the earlier transient cache-step infra kill, not a code bug. So these next-steps are precision refinements, not blockers.

I grounded the list in this session's actual changes (highest confidence) rather than guessing at the broader codebase — that deeper sweep is what the `#code-review` preset is for. Added three items to `agent:backlog` in priority order:

1. **`#logsigfp`** (highest — a real false-positive risk I introduced) — the new classifier checks `contains("err!")` / `contains("err_pnpm")` as bare substrings, so a benign line (prose, a URL, a path) containing those fragments would be misflagged as an error signal. Anchor them to token boundaries.
2. **`#logfixturecov`** — the fixture gate currently only proves false-negatives (`required_signals`). Add `forbidden_signals` cases so the gate also proves the classifier does NOT flag benign lines — this locks in the `#logsigfp` fix.
3. **`#tsinflight`** `[recommended]` — 16 unrelated in-flight files (graph-db/surrealdb/token-gate) are uncommitted; session-check keeps flagging partial-staging. Land or revert them in their own cycle so the committed tree verifies cleanly.

**Considered but NOT filed:** the line-family crate-name fold (`<word> v1.2.3` → `<name> <ver>`) applies beyond cargo, but that is defensible generic near-duplicate folding (the lines genuinely differ only by name+version), so I did not file it as a bug.

**Top of backlog: `#logsigfp`.**
<!-- agent:boundary:b481711f -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
<!-- /agent:queue -->
<!--
https://github.com/chopratejas/headroom
-->

## Backlog

<!-- agent:backlog priority -->
- [ ] [#logsigfp] Tighten log-digest npm/pnpm error classification. The new classify_generic_signal in packages/tsift-digest/src/log_digest.rs uses unanchored substring checks (contains err! and contains err_pnpm) that can flag benign lines (prose, URLs, paths) as error signals. Anchor them to token boundaries: match an npm/yarn ERR! token and an ERR_PNPM_ prefix at a word boundary. Add a regression test.
- [ ] [#logfixturecov] Harden the log-digest fixture gate against false positives. Add forbidden_signals cases to fixtures/log-digest-token-savings.json proving benign lines (prose mentioning err!, a non-cargo line with a version token) are NOT classified as error signals. Currently the gate only exercises false-negatives (required_signals); this locks in the logsigfp precision fix.
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
