---
agent_doc_session: tsift-v0.1
resume:
  claude: e045956e-575c-4573-b1ed-1371613ceed4
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

### Re: Fix the open tsift issues — opus-5

> **Queue prompt:**
>
> /goal Please fix the open issues in https://github.com/btakita/tsift/issues

All six open issues fixed and pushed as `ae31e36`. They are one family: each is a
case where tsift answered **confidently and wrong**, which is the worst failure
shape for a tool whose own instructions say to prefer it over `grep` and to read
full source only when it is insufficient.

**#5 — Go was never indexed.** The report's suggested fix was to drive the
indexer's language set from the `ast-grep languages` table, but that would have
collapsed a deliberate distinction: rewriting needs a *parser*, indexing needs
*tag queries*. Go was in the structural-only tier by design — the bug is that Go
had been sitting there while being a major language. So I promoted it properly:
symbol and call queries, an identifier-node-kind set, an entry in the indexed
semantic-edit tier, and conformance/rename fixture rows. Go's rename narrowing is
the interesting part — the grammar spells a struct field declaration, a field
read, a method name, and a package-qualified reference all as `field_identifier`.
A `field_declaration` name is never a symbol-query capture so it is ruled out
outright; for a selector, the receiver settles it by checking this file's imports.

The second half of #5 mattered more than the first. Skips were invisible
everywhere, so **any** unsupported language reads as a healthy index. The walk now
counts what it drops, `index` reports `skipped: N (unsupported extension)` with a
per-extension breakdown, and `status` prints a `language coverage:` block:

```
language coverage:
  scope go-tool: indexed 8 of 26 walked files — skipped .go 7, .json 6, .txt 5
```

Gated on the skips being a quarter or more of the walk *and* the dominant
extension costing three files or more, so a stray `.txt` beside 600 sources does
not grow a warning. There is a test for each direction.

**#6 — the query-shape dependency.** You were right that the error message is
good and the problem is that it does not fire consistently. Workspace roots with
no shared root index now federate for **every** strategy, which is what the
`exact` path already did. Auto-federation reuses the target resolver's own
precedence, so an explicit `--scope`, an inferred submodule or cargo package, an
agent-doc task path, or a real root index all still win — the auto-federate check
returns true only where the resolver used to `bail!`. A workspace whose every
scope opts out of federation now fails closed saying that, instead of returning
silent empty results.

**#7 — `explain` and `graph` at a workspace root.** Both take `--federated` and
resolve automatically. Resolution walks the federated scoped indexes for a
*definition*, falls back to a scope that only calls the symbol (so `graph
--callers` still answers), and breaks ties deterministically. When several scopes
match it answers from the first and names the others on stderr, so you narrow in
one step rather than guessing. A symbol no scope defines fails with the list of
scopes searched. I took your scope-down: cross-scope call edges stay out, and
resolution reports which scope answered.

**#8 — workspace instruction drift.** Both halves. `init --workspace` walks the
same scope list `status` walks and names every tracked file it writes per scope;
harness integrations (`--codex`, `--opencode`) stay at the root you invoked them
from, since only the Code Navigation block is what `status` reports on and what a
submodule-local harness loads. `status` reports drift per scope in the shape you
sketched, and — this is the part that closes the loop — scope drift now reaches
the `run:` recommendation, so a superproject whose own block is current still
recommends `tsift init --workspace`. Your opt-out is `instructions = false` under
a scope's `.tsift/config.toml` override; an opted-out scope is omitted rather
than reported `missing`.

**#9 — the flat 0.5000.** Your diagnosis was exact: F1 gives a single-tag symbol
perfect recall, so every one-of-three match lands on the same number and "three
matches at the same score" reads as ambiguity instead of noise. All three of your
suggested fixes are in. Coverage floor: half the query's tags, rounded up,
enforced in SQL so the candidate `LIMIT` is not spent on rows that will be
dropped. Scoring: F0.5, so one-of-three lands near `0.38` and two-of-three near
`0.71`. Keywords: a query whose every tag is a language keyword skips tag matching
and is answered lexically — exact-name matching still applies, so a symbol
literally named `def` is still found. When nothing clears the floor the section is
empty rather than padded.

**#10 — markdown pseudo-symbols.** Document files report `touched_headings` and
leave `touched_symbols` empty; only headings, not list items or fenced-block
languages, because a sentence fragment is not a structural summary. The report
carries `headings_touched` apart from `symbols_touched`, so `40 touched symbols`
can no longer mean a README grew sections — and that separation is also what stops
doc entries displacing real symbol churn in a mixed diff. The silent clip you
caught was a bare `.chars().take(96)` in `tsift-md-ast`; it marks the clip now.

**On the tests.** Four existing tests asserted behavior these fixes deliberately
reverse, and two coverage gates caught the Go promotion before I did — the rename
conformance table and the indexed-language conformance table both refused to pass
with a new indexed language and no fixture row. That is the table-as-unit-of-
coverage design working. I rewrote the four superseded tests to assert the new
contract rather than deleting them, and moved the structural-only exemplar from
Go to Java.

`make check` green: 67 suites, 1764 passed, 0 failed, clippy `-D warnings` clean.
CI for `ae31e36` finished green before the release commit went out. Specs updated
in `search-navigation.md`, `digests-sessions.md`, `ast-edits.md`,
`structural-patterns.md`, and `architecture.md`.

### Re: release + publish + install — opus-5

> **Queue prompt:**
>
> release + publish + install

`0.1.83` is cut, published, and installed. Publication is proven, not assumed.

**Cut.** `0528051`, then annotated tag `v0.1.83`. All 26 Rust packages plus
`Cargo.lock`, the `opencode-tsift` npm package, and `VERSIONS.md`. Instruction
surfaces were regenerated at the new version rather than hand-bumped.

One thing that needed catching: the packaged
`packages/opencode-tsift/commands/` copies are the plugin's *install source*, and
`tsift init --opencode` only writes `.opencode/commands/`. Shipping the npm
package would have installed 0.1.82 command bodies from a 0.1.83 plugin. I
regenerated them from a clean checkout and asserted all 15 are byte-identical to
`tsift init --opencode` output.

**Verified before tagging.** `make check` at 0.1.83: 67 suites, 1764 passed, 0
failed, clippy clean, npm pack dry run reporting `opencode-tsift-0.1.83.tgz`.

**Published — all three facts checked directly.** [Release
v0.1.83](https://github.com/btakita/tsift/releases/tag/v0.1.83) carries Linux,
macOS ARM, and Windows archives with `.sha256` files. All **26** crates report
`0.1.83` as `max_version` on crates.io — queried per crate, because a green
publish job is not proof of a published crate. npm reports `opencode-tsift`
`0.1.83`. Release run
[32676784588](https://github.com/btakita/tsift/actions/runs/32676784588) is green
across all 7 jobs.

**Installed.** `cargo install --path . --locked`; `tsift --version` reports
`tsift 0.1.83`, and `tsift status .` reports `instructions: current (v0.1.83)`
against a fresh index. The submodule is clean and synchronized with
`origin/main`.

Unlike last cycle I held this open for the publish rather than pinning the queue
head — the crates.io job alone ran about 25 minutes after the other six finished.
Two follow-ups are in the backlog: `communities` and `path` are the two workspace
commands #graphfed did not reach, and `cargo install --locked` warns about a
yanked transitive `spin v0.9.8`.
<!-- agent:boundary:cd881543 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
- do [#wsfedrest]
- do [#spinyank]
<!-- /agent:queue -->
<!--
https://github.com/chopratejas/headroom
-->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#wsfedrest] `communities` and `path` still fail closed at a workspace root with no shared root index. #graphfed gave `explain`/`graph` symbol-driven scope resolution; neither of these has a single symbol to resolve from (`communities` reads the whole index, `path` spans two symbols whose scopes may differ). Decide: federate `communities` per scope and merge, resolve `path` only when both endpoints land in one scope, or document the requirement in the generated Code Navigation block so the recommendation and the capability agree.
- [ ] [#spinyank] `cargo install --locked` warns that `spin v0.9.8` in Cargo.lock is yanked from crates.io. Harmless today, but a fresh `--locked` build of a published tag will keep warning. Refresh the transitive pin and re-run `make check`.
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
