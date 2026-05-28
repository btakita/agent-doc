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
queue_active: true
---

## Status

<!-- agent:status patch=replace -->

OpenCode npm package path code-complete and pushed. #gopencode-install and #gopencode gated on npm registry auth to publish opencode-tsift. 5 of 7 backlog items completed: integration tests, permission docs, transcript verify, smoke session, README expansion.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260528-222321.md`*

Compacted content:
- Archived 2 response topic(s): #step-4-graph; #step-6-resolution
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ What are #next-steps to split out crates?

### Re: #next-steps for crate split — opus-4-7

**Current state.** The workspace already publishes 8 crates (`tsift-core`, `tsift-sqlite`, `tsift-libsql`, `tsift-graph`, `tsift-algorithms`, `tsift-resolution`, `tsift-tokensave`, `tsift-cli`). Root `tsift` still owns ~25k LOC across 27 modules. `SPEC.md` describes the *target* layout but root has not been drained yet.

**Remaining modules grouped by cohesion + dependency direction:**

| Proposed crate | Modules | LOC | Depends on |
|---|---|---|---|
| `tsift-sim-world` | sim_world | ~390 | tsift-core |
| `tsift-digest` | diff_digest, log_digest, metric_digest, test_digest | ~3.7k | tsift-core (emitters only) |
| `tsift-index` | index, walk, config, init | ~4.7k | tsift-core, tsift-sqlite, tsift-graph |
| `tsift-session` | session_cost, session_digest, session_review | ~7.1k | tsift-index, tsift-digest |
| `tsift-quality` | audit, lint, perf_gate, dci_benchmark, runtime_churn | ~3.7k | tsift-index, tsift-graph, tsift-algorithms |
| `tsift-search` | sift, summarize, impact, tagpath_adapter | ~3.2k | tsift-index, tsift-graph, tsift-resolution |

**Recommended order** (bottom-up by dependency, lowest blast radius first):

1. **#split-sim-world** — smallest + most isolated; validates the extraction pattern with minimal risk.
2. **#split-tsift-digest** — high-cohesion emitter group; few inbound deps from other root modules. Sets schema/serialization conventions reused by session.
3. **#split-tsift-index** — foundation for session/quality/search. Biggest dependency hub remaining in root; landing it unlocks the larger splits.
4. **#split-tsift-session** — largest LOC pool (~7.1k). Depends on digest + index, so sequence-after these.
5. **#split-tsift-quality** — gate surfaces (audit/lint/perf_gate). Independent of session; can land in parallel with #4 if index is stable.
6. **#split-tsift-search** — search/summarize/impact + tagpath_adapter. Final business-logic extraction.
7. **#demote-root-tsift** — once root `src/lib.rs` only re-exports, demote to thin shim or remove entirely; preserve `cargo install tsift` entrypoint.

**Also flagged:** `tsift-cli` is already 39k LOC in a single `lib.rs` — once root is drained, the next problem becomes the CLI monolith itself (`#split-tsift-cli-subcommands`, recommended).

**Cross-cutting checklist per split:**
- update `SPEC.md` Architecture block to mark the move from root → new crate
- add `[workspace.members]` entry and seed `lib.rs` with re-export shim from root for backward compat during transition
- thread feature flags (`lang-rust`, `backend-libsql`, etc.) through the new crate's `Cargo.toml` if it touches gated code
- run `cargo install --path .` + smoke `/tsift-status` after each split to verify no command path broke
- bump root `tsift` version on each landing (per release conventions) and update `VERSIONS.md`

**Top of backlog after this update:** `#split-sim-world`.

### Re: do [#split-sim-world] — blocker, gating for direction — opus-4-7

**Halting the queue.** I scoped #split-sim-world and the original backlog framing was wrong — the module is not a clean leaf. Gating this item and stopping the auto-loop before touching the remaining 7 split directives, because the same coupling concern probably re-surfaces for #split-tsift-digest / #split-tsift-session and the right architectural call should be a conversation.

**What I found:**

1. **`src/sim_world.rs` is currently orphaned.** It is NOT declared in `src/lib.rs` (root `tsift`). Root lib.rs lists every other module (`audit`, `config`, …, `walk`) but omits `sim_world`. So as it stands the file does not compile against any build target — it is dead code.

2. **It went stale during the tsift-cli extraction.** Commit `1786d0d Extract tsift-cli crate` moved `rewrite_command` into `tsift-cli` (where it is `pub(crate)`). `src/sim_world.rs:1` still imports `use crate::{init, rewrite_command, session_digest, status};` — `crate::rewrite_command` no longer exists at that path. The recent commit `580f5f1 test: run all default tests` only nudged a single line; the orphaning was never repaired.

3. **It is tightly coupled to private/test-only symbols in two crates:**
   - `tsift-cli::rewrite_command` — `pub(crate)`, lives in `packages/tsift-cli/src/lib.rs:37769`
   - `tsift::session_digest::extract_prompt_targets_from_text_block` — `#[cfg(test)] pub(crate)` in `src/session_digest.rs:899`
   - `tsift::init::{InstructionStatus, TSIFT_VERSION}` and `tsift::status::check_status` — already `pub`

   Two of those four targets are deliberately not part of either crate's public API. Promoting them to `pub` solely to support an external `tsift-sim-world` test crate inverts the usual rule (don't widen visibility for tests).

**Three options:**

| Option | What | Trade-off |
|---|---|---|
| **A.** Delete `src/sim_world.rs` | Acknowledge it has been broken/dead since the tsift-cli extraction and drop it. | Lose the deterministic sim coverage. Smallest change. Honest about current state. |
| **B.** Re-home tests adjacent to their targets | Move session/status sim-cases into `tsift` (under `#[cfg(test)] mod sim_world;` re-added to `src/lib.rs`) and the `rewrite_command` sim-cases into `packages/tsift-cli/tests/sim_world_rewrite.rs`. Keep `pub(crate)` boundaries; expose only what crate-internal tests already see. | Restores coverage. No new crate, but no public API widening either. Closes #split-sim-world by relocating, not extracting. |
| **C.** Force a `tsift-sim-world` crate per original backlog item | Promote `rewrite_command` and `extract_prompt_targets_from_text_block` to `pub`, add `packages/tsift-sim-world` depending on both, wire workspace member. | Widens public API for test-only callers. Adds one more workspace member. Matches the queue directive literally. |

**Recommendation: B.** It restores coverage that the tsift-cli extraction silently broke, keeps API surface narrow, and avoids re-using `tsift-sim-world` as a new crate name when the actual code is just test-relocation. After B lands, the backlog item closes as "done by relocation" and the remaining 7 splits (digest/index/session/quality/search/demote/cli-subcommands) — which DO look like genuine module-extraction targets — proceed unchanged.

**What I need from you:**

- Confirm A / B / C (or a hybrid).
- Decide whether to keep the rest of the queue active. If you pick **B**, I can run it next turn and the remaining 7 directives stay valid. If you pick **C**, I should re-scope each of the other 7 items for the same coupling concern before letting the auto-loop drive them.

**Why I am gating instead of pushing through:** the existing CLAUDE.md guidance for tsift says "All public logic tested: `classify_task`, `apply_edit_op`, `rewrite_command`, SQL helpers all have unit tests." That convention covers the rewrite_command tests, but it does not mandate cross-crate public exposure. Picking C without you signing off would silently widen the tsift-cli public API.
<!-- agent:boundary:00656bef -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to make the graph faster?
---
What are #next-steps to make the graph + neighborhoods faster + more effective?
-->

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- ~do [#split-sim-world]~
- do [#split-tsift-digest]
- do [#split-tsift-index]
- do [#split-tsift-session]
- do [#split-tsift-quality]
- do [#split-tsift-search]
- do [#demote-root-tsift]
- do [#split-tsift-cli-subcommands]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#split-tsift-digest] Extract diff_digest + log_digest + metric_digest + test_digest (~3.7k LOC) into tsift-digest crate — high-cohesion emitter group with shared serialization, schema versioning, and pruning patterns; minimal cross-deps
- [ ] [#split-tsift-index] Extract index + walk + config + init (~4.7k LOC) into tsift-index crate — foundation that tsift-session, tsift-quality, tsift-search will depend on; pulls in fs4, ignore, blake3, flate2 deps; carries SQLite schema bootstrap
- [ ] [#split-tsift-session] Extract session_cost + session_digest + session_review (~7.1k LOC, largest pool) into tsift-session crate — depends on tsift-digest + tsift-index; biggest single LOC reduction in root tsift
- [ ] [#split-tsift-quality] Extract audit + lint + perf_gate + dci_benchmark + runtime_churn (~3.7k LOC) into tsift-quality crate — quality-gate surfaces depend on tsift-graph + tsift-index; isolates gate evaluation from emitter and session crates
- [ ] [#split-tsift-search] Extract sift + summarize + impact + tagpath_adapter (~3.2k LOC) into tsift-search crate — depends on tsift-index + tsift-graph; covers search ranking, AST-aware summaries, impact analysis, and tagpath annotation surface
- [ ] [#demote-root-tsift] After all module splits land, demote root tsift crate to a thin re-export shim (or remove lib.rs entirely) — leave only the workspace root + binary metadata; verify external consumers (cargo install tsift) keep working
- [ ] [#split-tsift-cli-subcommands] [recommended] tsift-cli is ~39k LOC in a single lib.rs — propose per-command module split (cmd_search, cmd_index, cmd_graph, cmd_explain, etc.) once root tsift is drained, to make command surface independently testable
- [ ] [#npm-auth-setup] Configure NPM_TOKEN secret + TSIFT_ENABLE_NPM_PUBLISH=true variable in GitHub repo settings
- [ ] [#npm-publish-verify] After auth configured, tag release and verify publish-opencode-plugin CI job succeeds and opencode-tsift appears on npmjs.org
- [ ] [#full-e2e-verify] In fresh project: opencode plugin opencode-tsift && opencode then /tsift-status. On success mark #e2e-install done.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#split-sim-world] Extract sim_world (~390 LOC) into tsift-sim-world crate — smallest, most isolated module; low-risk pattern validation for the remaining splits; keeps sim/test helpers out of the prod-path tree
- [/] [#full-e2e-verify] In fresh project: opencode plugin opencode-tsift && opencode then /tsift-status. On success mark #e2e-install done.
- [/] [#npm-publish-verify] After auth configured, tag release and verify publish-opencode-plugin CI job succeeds and opencode-tsift appears on npmjs.org
- [/] [#npm-auth-setup] Configure NPM_TOKEN secret + TSIFT_ENABLE_NPM_PUBLISH=true variable in GitHub repo settings
- [/] [#e2e-install] Verify end-to-end install: fresh opencode project + opencode plugin opencode-tsift + /tsift-status
<!-- /agent:review -->
###
<!--
Can we increase max-hop caps?
Could FalkorDB help with performance?
-->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
