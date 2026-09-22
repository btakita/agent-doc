---
agent_doc_session: tsift-v0.1
resume:
  claude: 03da93b9-58a8-4aa3-b530-ca5405b2ea0b
  codex: 01a0ca63-49bb-7040-a638-a80239e1503e
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#advance-review': Go through the review items. Add actionable backlog items to complete or advance the review items.,
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#bugs-observed': Please add any bugs from observed behavior to the backlog.,
  '#code-review': Please review tsift codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. '#follow-up-backlog'
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: go
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260922-180951.md`*

Compacted content:
- Archived 16 response topic(s): fpe.md runaway tsift task; agent-doc commit and push check; team-friendly tsift instructions; 13 more
- Prior summary/context: prior compacted content: Archived 8 response topic(s): Session resumed; Issue #25 fixed and v0.1.97 published; tsift ...

### Re: #ciwalflakeregress — regression coverage for the search lock-hook readiness bound — opus-5 · 2026-09-22T14:18-04:00

> **Queue prompt:**
>
> do [#ciwalflakeregress]


Done. `src/tsift` `bc00a7e`, superproject `69ffb6e20`.

**The /tmp blocker was real but misattributed.** It was not cargo needing space — it was 148 `/tmp/agent-doc-native-<pid>` directories, 51G, the leaked agent-doc test-fixture roots orphaned to PID 1. 147 had dead PIDs; removing those took /tmp from 99% to 45% (52G free). The one live PID was left alone. Nothing else in /tmp was touched — the large `fpe-*` / `haiven-*` trees are other sessions' worktrees.

**What the commit adds.** `SearchPostPrecheckLockHook` now carries a `setup_delay` that the holder thread sleeps *before* opening SQLite, so the latency lands inside the readiness window — the same place a contended runner loses its time, not merely somewhere adjacent to it. Existing installers pass `Duration::ZERO` and are unchanged. The new `search_cmd_wal_fallback_survives_setup_latency_past_the_old_readiness_cap` installs the WAL hook with 1.5s of that latency and asserts, *before* running the search, that the delay is both above the pre-fix 1s cap and below the 30s bound; without those two assertions the test could keep passing while having quietly stopped reproducing anything.

**Verified by mutation, not by argument.** Reverting only the readiness wait to `Duration::from_secs(1)` — the exact pre-2171338 state — reddens the new test:

```
WAL snapshot fallback failed under 1.5s of injected setup latency:
waiting for search post-precheck lock hook
Caused by: timed out waiting on channel
```

Under that same mutation the pre-existing `search_cmd_uses_wal_snapshot_fallback_when_lock_appears_after_precheck` **stays green**. That gap is precisely the coverage this adds: the original test never held the bound, which is why hand-editing was the only way to check it. Restoring the bound returns both to green. The mutation was applied and restored from a scratch copy with `touch` after each swap, so no run read a stale build.

`make check`: exit 0, 67 suites, **1876 passed, 0 failed** (1875 before — the delta is this test). `cargo build --release` + `cargo install --path .` done, `tsift 0.1.100` installed.

**CI is queued, not green yet** — run `35766280628` was 4s old at push time. The preceding run on `2171338` is green. Local evidence above is what this closeout rests on.

**One thing worth knowing for future sessions:** `lint::tests::ambient_tsift_state_dir_is_not_a_workspace_root` fails if `TMPDIR` points anywhere under `$HOME`. The test builds a fake home in a tempdir and declares only that tempdir ambient, so the ancestor walk continues past it and finds the real `~/.tsift`, resolving the root to `/home/brian`. That is arguably correct behavior rather than a bug — `$HOME` genuinely is `.tsift`-marked and was not declared ambient — but it means a `TMPDIR` override is not a safe way to work around a full `/tmp` for this suite. The full run above used the default `/tmp` after the reclaim.

### Re: Response

This pane is the failed `tsift.md` launch itself: it never received agent-doc’s required preflight contract, so the workflow must fail closed.

The original API session is actively implementing the systemic route-layout fix. No changes were made from this pane. Once that session lands and installs the fix, rerun **Run Agent Doc** on `tsift.md`.

### Re: release + publish — gpt-5 · 2026-09-22T16:03-04:00

> **Queue prompt:**
>
> release + publish

Prepared and fully verified `tsift` **v0.1.101** at commit `e47c34870` with local tag `v0.1.101`. The tag will be pushed immediately after this binary-owned document closeout; that push triggers the repository's trusted-publishing workflow for the Rust workspace crates, the `opencode-tsift` npm package, and GitHub release assets.

This release includes the post-0.1.100 harness-skill linking and version-drift work, plus a release-blocking concurrency fix discovered by the gate: managed skill and runbook refreshes now replace files atomically, so parallel `status --fix` / `init` processes cannot expose a truncated marker block. The regression proves an update replaces the inode rather than truncating in place and preserves existing permissions.

Verification completed against the exact 50-path release candidate:

- focused atomic-update regression: passed;
- previously failing `status_fix_refreshes_stale_index_before_reporting_json`: passed;
- full `make check`: exit 0, zero failure groups;
- `cargo build --release`: passed;
- `cargo install --path . --locked`: passed; installed `tsift 0.1.101`;
- `tsift status`: fresh index and current `v0.1.101` instructions;
- added-line private-name and whitespace checks: clean;
- latest pre-release CI on `main` (`35766280628`): green.

Separate operator warning: IntelliJ still has superseded agent-doc plugin bytes mapped. Restart IntelliJ or reopen the document tab to load the installed jar.
<!-- agent:boundary:d4c17bb5:tsift -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
- 🚧 release + publish
<!-- /agent:queue -->
<!--
https://github.com/chopratejas/headroom
-->

## Backlog

<!-- agent:backlog priority queue -->
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

<!-- agent:notes -->
• Ran tsift rewrite --run 'git log -5 --oneline --decorate'
└ tsift rewrite: no rewrite: no supported tsift rewrite matched this command; `--run` executes only rewritten commands; run the original command directly if intended

#spec-test-build-install-commit-push
---
route duplicate prompt residue guard failed: failed to parse components: unclosed component: `agent:notes` without matching close
<!-- /agent:notes -->
