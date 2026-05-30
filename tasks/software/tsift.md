---
agent_doc_session: tsift-v0.1
agent: codex
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
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260530-140610.md`*

Compacted content:
- Archived 4 response topic(s): agent-doc domain boundary; #graph-neighborhood-budget; #token-action-coverage; 1 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
What are #next-steps to extract tsift crates?

### Re: next steps to extract tsift crates — gpt-5

The extraction is mostly past the first split: `src/tsift/Cargo.toml` already has a `packages/tsift-*` workspace, and the root `src/tsift/src/*.rs` surface is now just re-export shims. The highest-value next step is to remove the remaining runtime reliance on that shim from `tsift-cli`; right now `packages/tsift-cli` still depends on `tsift` and many CLI call sites use `tsift::...`, so the root crate is still part of the CLI implementation path instead of only a compatibility facade.

I added the follow-up backlog in priority order:

1. `#tsiftcli-direct-crates` — make `tsift-cli` import sibling crates directly and leave root `tsift` as a compatibility re-export crate.
2. `#tsift-dormant-crates` — make `tsift-tokensave` and `tsift-algorithms` intentional in the public/CLI surface, or explicitly park them so the spec, workspace, and dependency graph agree.
3. `#tsift-crate-publish-preflight` — normalize extracted-crate version/readme/publish metadata and prove publish order with check/test plus dry runs.

Top backlog item: `#tsiftcli-direct-crates`.

No code changed in this cycle; I only inspected the crate layout and added backlog items.

### Re: queued next steps closeout — gpt-5

This queued copy is the same `#next-steps` prompt answered immediately above. The backlog is already updated in priority order, and the top item remains `#tsiftcli-direct-crates`.

❯ re [#tsift-dormant-crates]: Are some crates not used?

### Re: dormant crates usage — gpt-5

Yes. From the current workspace graph, `tsift-tokensave` is a real workspace crate with tests, but it has no reverse dependency from the root `tsift` crate, `tsift-cli`, or another workspace crate. It is documented in `SPEC.md`, but operationally it is parked.

`tsift-algorithms` is in a softer state: `tsift-cli/Cargo.toml` declares it as a dependency, so `cargo tree --workspace -i tsift-algorithms` shows it reachable through `tsift-cli`; however, source search only found the dependency declaration and spec text, not actual Rust call sites. So it is probably an unused dependency rather than a wired feature.

That is why `#tsift-dormant-crates` should stay open: the task is not just to delete dead code. It should decide, crate by crate, whether to wire the surface into CLI/root exports or explicitly park/remove the crate so the spec, manifests, and dependency graph all say the same thing.

No code changed in this cycle.
re [#tsift-dormant-crates]: Is there functionality in tsift-tokensave and tsift-algorithms that should be used?
<!-- agent:boundary:23f17720 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to make the graph faster?
---
What are #next-steps to make the graph + neighborhoods faster + more effective?
-->

<!-- agent:queue auto -->
- re [#tsift-dormant-crates]: Is there functionality in tsift-tokensave and tsift-algorithms that should be used?
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#tsiftcli-direct-crates] Remove tsift-cli's dependency on the root tsift shim by importing sibling tsift-* crates directly, leaving the root crate as backwards-compatible re-export surface only.
- [ ] [#tsift-dormant-crates] Make dormant extracted crates intentional: wire or explicitly park tsift-tokensave and tsift-algorithms so workspace membership, CLI usage, root re-exports, and spec language agree.
- [ ] [#tsift-crate-publish-preflight] Normalize extracted-crate release metadata and version policy: align split-crate versions/readmes/publish settings, then run cargo check/test plus cargo publish --dry-run in dependency order.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
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
- [ ] [#npm-auth-setup] Configure NPM_TOKEN secret + TSIFT_ENABLE_NPM_PUBLISH=true variable in GitHub repo settings
- [ ] [#npm-publish-verify] After auth configured, tag release and verify publish-opencode-plugin CI job succeeds and opencode-tsift appears on npmjs.org
- [ ] [#full-e2e-verify] In fresh project: opencode plugin opencode-tsift && opencode then /tsift-status. On success mark #e2e-install done.
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
