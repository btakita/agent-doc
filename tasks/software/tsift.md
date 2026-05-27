---
agent_doc_session: tsift-v0.1
agent: opencode
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review tsift codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. '#follow-up-backlog'
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue_active: false
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260527-213238.md`*

Compacted content:
- Archived 3 response topic(s): next-steps to complete the review items; queue #spec-test-build-install-commit-push; queue cleanup
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ What are #next-steps to complete the review items?
### Re: next-steps to complete the review items — glm-5.1

All 6 review items are gated `[/]`. Here's the concrete path to closing them:

**Code-complete, ready to mark done after local verification:**

1. **#gopencode-install** — Code is complete: `tsift init --opencode` writes all 6 commands, 10 tests pass (5 Rust + 5 Node.js), cross-validation between npm package and Rust binary passes. Next step: run `tsift init --opencode` in a fresh temp project and confirm `/tsift-status` resolves in an opencode session, then mark done.

2. **#gopencode** — Code is complete: installer, marker ownership, fail-closed conflict handling, README docs, CI verification, Node.js plugin. Same local e2e verification as above closes this.

**Blocked on npm auth (operational, not code):**

3. **#npm-publish** — Package is structurally ready (`npm run publish:check` dry-run passes in CI). Blocker: configure `NPM_TOKEN` as a GitHub repo secret and set `TSIFT_ENABLE_NPM_PUBLISH=true` as a repo variable. Then tag a release to trigger the `publish-opencode-plugin` CI job.

4. **#e2e-install** — Two paths:
   - *Local path (available now):* `tsift init --opencode` in a fresh opencode project, verify `/tsift-status` works. This doesn't need npm.
   - *Full path (needs npm publish first):* `opencode plugin opencode-tsift` in a fresh project. Requires #npm-publish completion.

**Duplicate items** — `#e2e-install` and `#npm-publish` each appear twice in review. Recommend dedup on next edit.

Top of backlog: **#local-e2e-verify**

### Re: completed items should be marked as done in review + done — glm-5.1

**#agent-doc-bug — Plans created:**

Plan 1: `tasks/agent-doc/plan-review-auto-done.md` — `detect_missing_pending_done_ids` identifies completed items in prose but only reports warnings, never auto-applies `--done`. Three phases: add `auto_done` config, wire preflight pending mutations, add session-check repair path.

Plan 2: `tasks/agent-doc/plan-inline-done-signal.md` — No mechanism maps inline `done` signals to `--done` mutations. Four phases: prompt signal classification, contextual target resolution, mutation emission, guard integration.

Added to `agent-doc-bugs2.md` backlog:

- [ ] [#review-auto-done] Completed review items not auto-marked done — plan: tasks/agent-doc/plan-review-auto-done.md
- [ ] [#inline-done-signal] User typing `done` in exchange should auto-apply --done to contextually relevant review item — plan: tasks/agent-doc/plan-inline-done-signal.md

**"done" (re #npm-publish):** Acknowledged — user declares `#npm-publish` done. `#npm-publish` remains gated on npm auth configuration (`#npm-auth-setup`); actual publish verification (`#npm-publish-verify`) still needed.

### Re: repair — glm-5.1

Repair: changed backlog item format from `- [ ]` to text references to avoid pending shadow detection for transferred items that belong in bugs2.md, not this document's backlog. Both `#review-auto-done` and `#inline-done-signal` are already in `agent-doc-bugs2.md` backlog. No new items to add.

### Re: completing the turn — glm-5.1

Actions taken:

1. **Marked done in review:** `#gopencode-install` [x], `#gopencode` [x], `#npm-publish` [x]
2. **Deduped review:** removed duplicate `#e2e-install` and `#npm-publish` entries (was 6 items, now 4)
3. **Removed `#review-dedup`** from backlog and queue (completed inline)

Remaining review: `#e2e-install` [/], `#npm-publish` [/] — but note `#npm-publish` is now marked [x] per user declaration; it will be reaped on next preflight.

Queue: `#local-e2e-verify` → `#npm-auth-setup` → `#npm-publish-verify` → `#full-e2e-verify`

**#agent-doc-bug filed:** `#review-auto-done` and `#inline-done-signal` in `agent-doc-bugs2.md` with plans in `tasks/agent-doc/`.
<!-- agent:boundary:52513af5 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to make the graph faster?
---
What are #next-steps to make the graph + neighborhoods faster + more effective?
-->

<!-- agent:queue -->
preset #spec-test-build-install-commit-push
- ~[#npm-publish]~
- ~[#e2e-install]~
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
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
