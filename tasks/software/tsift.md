---
agent_doc_session: tsift-v0.1
resume: 01a02767-e76b-7183-9616-e762301dee47
agent: claude
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260822-020642.md`*

Compacted content:
- Archived 4 response topic(s): release + publish; How do we fix tsift-astgrep publishing?; reactive projection acceptance; 1 more
- Prior summary/context: prior compacted content: Archived 6 response topic(s): do [#registerextractfunction]; do [#extractfrommethod]; do [#e...

### Re: Fix tsift issues 1–3 — gpt-5

> **Queue prompt:**
>
> Fix https://github.com/btakita/tsift/issues/2


> **Queue prompt:**
>
> Fix https://github.com/btakita/tsift/issues/1


Implemented and committed all three issue fixes in the tsift submodule.

- **#1 — AWS Bedrock summarize:** extraction now resolves one reusable backend only for real cache misses. Hosted Claude providers run through authenticated, non-interactive `claude -p`; direct Anthropic keys retain the API transport. Missing or unusable credentials fail once, before the extraction walk, with a nonzero exit.
- **#2 — runbook location:** `.agent/runbooks/code-navigation.md` is canonical. `tsift init` migrates the legacy path while preserving unmanaged text, updates generated references, and is idempotent.
- **#3 — portable AGENTS boilerplate:** generated verification guidance is capability-based: `make check` or a supported `just` recipe, plus `gh` or `glab` only when the matching repository metadata and executable exist.

Verification is green: focused summarize/init/conformance tests, full `make check`, `cargo install --path .`, installed `tsift 0.1.80`, two-pass `tsift init .` idempotence, current `tsift status`, and the latest existing GitHub Actions CI run. The implementation commit is `2d018e2` (`Fixes #1`, `Fixes #2`, `Fixes #3`); its push follows this document closeout.

### Re: release + publish + install — gpt-5

> **Queue prompt:**
>
> release + publish + install

Released tsift `0.1.81` completely.

- Fixed the clean-runner clippy regression in SQLite semantic-vector decoding by using fixed-width byte arrays, with direct round-trip and invalid-payload regression coverage.
- Versioned all 26 Rust packages, the OpenCode npm package, generated instruction surfaces, and release history for `0.1.81`.
- Committed and pushed `3ca8655` (`release: 0.1.81 -- portable summarize and init`), then pushed the annotated `v0.1.81` tag.
- Verification is green: focused SQLite clippy/test gates, full `make check`, package-file checks for all 26 crates, the OpenCode npm publish dry run, `main` CI including live Convex acceptance, and the complete tagged Release workflow.
- Publication is proven: [GitHub Release v0.1.81](https://github.com/btakita/tsift/releases/tag/v0.1.81) is live with Linux, macOS ARM, and Windows binaries plus SHA-256 files; all 26 crates, including `tsift-sim-world`, are live on crates.io; and npm reports `opencode-tsift` `0.1.81`.
- Installed locally with `cargo install --path . --locked`; `tsift --version` reports `tsift 0.1.81`.
- Rebuilt the stale local agent-doc binary/native library before closeout; the installed version remains current at `0.35.258`.

The tsift submodule is clean and synchronized with `origin/main`. No additional backlog follow-up is needed for this release.

### Re: Fix tsift issue #4 — opus-5

> **Queue prompt:**
>
> Fix https://github.com/btakita/tsift/issues/4

Fixed and pushed as `690317f` (`Fixes #4`).

**The split.** 0.1.81's auto-fix bundled two unlike things. `apply_status_fixes` refreshed the tracked Code Navigation block and relocated the legacy runbook *before* touching the index, so a command whose help reads "Report index + summary status" left a two-file diff — one of them a deletion — in a version-controlled tree. Auto-fix now covers only the gitignored `.tsift/` state tsift owns: index refresh, workspace-scope rebuilds, cycle-packet cache eviction. Tracked-file rewrites moved into `apply_status_instruction_fixes`, reachable only from `tsift init` or the new `tsift status --fix-instructions`. Bare `status` reports `instructions: stale (... — run tsift init)` and folds `tsift init` into its `run:` line instead of writing.

The deprecated `--fix` is no longer a no-op — it now means exactly `--fix-instructions`, which is the behavior it had before 0.1.81. An explicit flag writing tracked files was never the surprise; the bare command doing it was. `--no-fix` still skips everything.

**Naming the writes.** `InitResult` carries the relocation as `migrated_runbook: Option<RunbookMigration>` rather than the bool it discarded, so both commands announce the move. Live repro against the installed binary, from the issue's own setup:

```
$ tsift status .                              # bare — the issue's repro
instructions: stale (v0.1.80 installed, v0.1.81 available — run tsift init)
  run: tsift init && tsift summarize --extract .
$ git status --short
?? .tsift/                                    # nothing tracked touched

$ tsift status --fix-instructions .
status fix: refreshing tsift instructions (tracked files)
status fix: moved runbooks/code-navigation.md -> .agent/runbooks/code-navigation.md
status fix: rewrote AGENTS.md (v0.1.80 -> v0.1.81)
status fix: created .agent/runbooks/code-navigation.md (v0.1.80 -> v0.1.81)
```

**The templates.** The injected block and the `/tsift-status` OpenCode command said `tsift status --fix` — tsift shipping instructions for its own deprecated flag. Both now say `tsift status`, and the block explains the actual contract: status repairs `.tsift/` and never rewrites tracked files; `tsift init` refreshes the instruction surface and names what it touches. The OpenCode plugin's load-time freshness check drops the flag too, so plugin load cannot dirty a tracked file. Your "worth a release check" note is now enforced, not remembered: `init::DEPRECATED_FLAG_USAGES` lists deprecated forms and a unit test asserts the `AGENTS.md` block, the managed runbook, and every generated OpenCode command body are free of them.

**Verification.** Five new CLI regressions, including the one that would have caught this — a bare `status` over a stale block plus a stale index must leave `AGENTS.md` byte-identical while still printing `status fix: refreshing index`. Full `make check` green (67 suites, 0 failed, clippy `-D warnings` clean), 8/8 `opencode-tsift` node tests, `cargo install --path .`, and the live repro above. Specs updated in `specs/search-navigation.md` (status section, injected block, OpenCode plugin) and `specs/graph.md` (cache eviction stays in the `.tsift/`-only class); `VERSIONS.md` has the unreleased entry.

Two things I did not do. Tsift CI for `690317f` is queued, not finished — local evidence is what the above rests on. And the workspace's `agent-doc` binary is stale by one commit (`35efb2804`), but `src/agent-doc` currently holds a large uncommitted tree from a concurrent session, so building it here would install someone else's in-progress state; leaving that to the supervisor's idle-boundary recycle.

### Re: release + publish + install — opus-5

> **Queue prompt:**
>
> 📌 release + publish + install

Release cut and tagged; install done; publication is in flight.

**Cut.** `a498777` (`release: 0.1.82 -- status stops rewriting tracked files`), pushed to `main`, then annotated tag `v0.1.82` pushed. That is the trigger — the release workflow is tag-driven and fails closed if the tag does not exactly match `Cargo.toml` `package.version`.

Versioned to 0.1.82: all 26 Rust packages plus `Cargo.lock`, the `opencode-tsift` npm package, and `VERSIONS.md` (the unreleased entry cut as `## 0.1.82`). Generated instruction surfaces were regenerated at the new version rather than hand-bumped — `AGENTS.md`, `.agent/runbooks/code-navigation.md`, and all 15 OpenCode command files in both `.opencode/commands/` and the packaged `packages/opencode-tsift/commands/` copies the release check compares them against.

One thing the regeneration surfaced: this repo's own `.opencode/commands/` was three files behind what `tsift init --opencode` produces — `/tsift-explain`, `/tsift-graph`, and `/tsift-symbol-read` had never been written here. They are in the release commit now.

**Verified before tagging.** Full `make check` at 0.1.82: 67 suites, 0 failed, `cargo clippy --workspace --all-targets -- -D warnings` clean, and the npm pack dry run reporting `opencode-tsift-0.1.82.tgz`. CI for the preceding fix commit `690317f` was green before the release commit went out.

**Install.** `cargo install --path . --locked`; `tsift --version` reports `tsift 0.1.82`.

**Publish is not yet proven.** [Release run 32661754044](https://github.com/btakita/tsift/actions/runs/32661754044) is in progress, currently in `verify release inputs`. The previous release took ~31 minutes end to end, so I am not holding this cycle open for it — the local evidence above is what this response rests on. I have a watch armed on the run's job results and will report the three publication facts when it lands: the GitHub Release with Linux/macOS-ARM/Windows archives plus `.sha256` files, all 26 crates on crates.io via OIDC trusted publishing, and `opencode-tsift@0.1.82` on npm. If a publish job fails, the crates.io step skips versions that already exist, so a re-run resumes rather than restarting.

Leaving the queue head pinned until that proof is in.
<!-- agent:boundary:cd881543 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
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
