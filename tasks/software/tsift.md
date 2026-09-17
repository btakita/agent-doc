---
agent_doc_session: tsift-v0.1
resume:
  claude: 7ec8a9b0-c6b3-46ca-b057-dfbb94db6d6d
  codex: 01a0ab82-e77c-7cd3-8b55-e2f23cc48f96
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260911-150553.md`*

Compacted content:
- Archived 8 response topic(s): Session resumed; Issue #25 fixed and v0.1.97 published; tsift compared with Graft; 5 more
- Prior summary/context: prior compacted content: Archived 6 response topic(s): tsift 0.1.96 released; Response; do [#fixcompactexchange]; 3 more
> **Queue prompt:** fpe.md had a tsift task running for 1hr 51min before I killed it. Fix it.

### Re: fpe.md runaway tsift task — gpt-5 · 2026-09-12T00:37-04:00

Fixed. This was an `agent-doc` dispatch-lifecycle race, not a tsift execution bug: a `Ready` projection consumed the in-flight receipt before the dispatched turn actually started, so the idle watcher admitted a duplicate long-lived task into the same composer.

The exact `fpe.md` trace was already fixed, committed, and pushed as agent-doc commit `b666a8d7` (`fix(dispatch): prove a dispatch's turn started instead of inferring it from Ready`). Dispatch receipts now move to `running` only after an observed `Busy` transition from `caller=dispatch`; `Ready` consumes only a proven-running receipt, with a bounded 120-second grace path for genuinely lost triggers. Unrelated supervisor `Busy` transitions cannot vouch for a dispatch.

The installed `agent-doc 0.35.367` binary was built at 00:06, after the fix commit at 23:35. I did not reinstall over the unrelated concurrent `0.35.368` source edits.

Verification is green:

- exact state-store regression for the 33-second premature-Ready race;
- lost-trigger grace regression;
- controller lifecycle/coalescing regression;
- structural Busy/Ready wiring regression;
- latest GitHub Actions CI run `34672029496` completed successfully.

No additional source changes were necessary in this cycle; all three fix-owned paths are clean.

### Re: agent-doc commit and push check — gpt-5 · 2026-09-12T00:40-04:00

> **Queue prompt:**
>
> do [#changecommitpush]


I made no `agent-doc` source change, so there is no new change of mine to commit or push. The `fpe.md` fix is already committed as `b666a8d7` and present on `origin/main`.

The three fix-owned paths remain clean. Local `agent-doc` `main` is currently one commit ahead at `8912cc44` with additional dirty editor files, but that commit and those edits were created concurrently by another session; I did not push or fold them into this task.

### Re: team-friendly tsift instructions — gpt-5 · 2026-09-15T18:57-04:00

> **Queue prompt:**
>
> do [#tsiftfriendlyteam]


No—tsift should not default to `AGENTS.override.md`.

Codex loads at most one instruction file per directory and prefers `AGENTS.override.md` over `AGENTS.md`. A generated root override would therefore hide the team's root instructions from Codex rather than augment them. It is also a Codex-specific mechanism, so it would make tsift's cross-harness behavior less consistent. This follows the [official Codex instruction-discovery contract](https://developers.openai.com/codex/guides/agents-md).

The better configuration is an explicit instruction deployment policy:

- `personal`: the team-friendly default for new installs when a repository already has its own instructions. Do not change tracked `AGENTS.md`, `CLAUDE.md`, or `.agent/` files. Install a user-scoped tsift skill with its bundled runbook and any user-local harness integration. `tsift status` may report this state but must not repair tracked instruction files.
- `shared`: the current behavior, enabled by an explicit repository opt-in. Maintain the small managed block in `AGENTS.md` plus the tracked `.agent/runbooks/code-navigation.md`.
- `off`: CLI-only operation with no instruction or hook installation.

Existing repositories that already contain tsift's managed markers should remain in `shared` mode until explicitly migrated, avoiding a breaking upgrade. The existing `[overrides.<scope>] instructions = false` setting is useful for skipping individual workspace scopes, but it does not solve root-level deployment or provide the personal skill path.

`AGENTS.override.md` can remain an advanced, explicit Codex-only escape hatch, but tsift should never select it automatically. If offered at all, initialization should warn or fail when a same-directory `AGENTS.md` exists unless the operator explicitly accepts that it will be masked.

The implementation should prove four invariants: personal/off modes create no tracked diffs; shared mode remains idempotent; migration removes only tsift-owned marker regions; and the personal skill/runbook works across supported harnesses. I recorded that implementation as recommended follow-up `#tsiftinstrpolicy`. No tsift source change was needed to complete this architecture decision.

### Re: skill-first tsift instructions — gpt-5 · 2026-09-15T19:01-04:00

Yes. A skill is the better default than modifying `AGENTS.*`.

For Codex, tsift can install a user-scoped skill at `~/.agents/skills/tsift/` or a repository-scoped skill at `.agents/skills/tsift/`. Codex initially sees the skill name and description, then loads the full `SKILL.md` only when explicitly invoked or when the task matches its description. That gives us progressive disclosure without changing a team's tracked instruction files. The current [OpenAI skill documentation](https://learn.chatgpt.com/docs/build-skills) explicitly supports both user and repository skill locations and implicit invocation.

The tradeoff is that a skill is conditional, whereas `AGENTS.md` is loaded before every task. Therefore the proposed modes should be:

- `personal` (default): install a user-scoped tsift skill and bundled runbook; make its description broad enough to trigger for code search, source reading, diffs, logs, and test output. Do not modify the repository.
- `shared`: install a checked-in repository skill for team-wide discovery. Add a minimal `AGENTS.md` router only if the team explicitly wants tsift to be mandatory and always-on.
- `off`: install no instruction integration.

We should not generate `AGENTS.override.md`: Codex selects only one instruction file per directory and prefers the override, so it can hide an existing team `AGENTS.md`, as documented in [OpenAI's `AGENTS.md` discovery rules](https://learn.chatgpt.com/docs/agent-configuration/agents-md).

For Claude Code and OpenCode, tsift can generate equivalent harness-native skill mirrors from the same bundled source. That keeps the policy skill-first across harnesses even though the discovery paths differ.

This sharpens `#tsiftinstrpolicy`: implement the skill as the primary integration, and retain `AGENTS.md` only as an explicit shared enforcement option. No source change was requested in this question-only cycle.
Yes—use a skill as the default integration:

- `personal`: user-scoped skill, no repository mutation.
- `shared`: checked-in repository skill; minimal `AGENTS.md` only for explicitly always-on enforcement.
- `off`: no integration.

Codex supports user/repository skills with implicit activation ([OpenAI skill docs](https://learn.chatgpt.com/docs/build-skills)). Avoid generated `AGENTS.override.md`, which can mask the team’s existing instructions ([discovery rules](https://learn.chatgpt.com/docs/agent-configuration/agents-md)).

The answer is captured in the live document, but the editor-native save receipt remains pending, so it is not committed yet. The [agent-doc skill](/home/brian/work/btakita/agent-loop/.codex/skills/agent-doc/SKILL.md) requires leaving the editor authoritative while its controller retries; I did not force disk or duplicate the response.

### Re: tsift repository skill released — gpt-5 · 2026-09-16T15:35-04:00

> **Queue prompt:**
>
> Make tsift use a skill instead of an `AGENTS.md`/`AGENTS.override.md` entry. release + publish

Released tsift 0.1.98.

`tsift init` now installs a repository skill at `.agents/skills/tsift/SKILL.md` with the detailed workflow in `.agents/skills/tsift/references/code-navigation.md`. It no longer creates or appends tsift guidance in `AGENTS.md`, `AGENTS.override.md`, or `CLAUDE.md`; migration removes only tsift-owned legacy marker regions and preserves surrounding project instructions. Existing legacy runbooks move into the skill reference, and an unmanaged same-path skill fails closed instead of being overwritten. `tsift status --fix-instructions` and workspace-scope initialization use the same skill-first path.

The generated skill follows progressive disclosure: its concise description and hot-path workflow trigger repository navigation, while detailed commands remain in the linked reference. This matches the repository skill location and implicit activation documented by [OpenAI's skill guide](https://learn.chatgpt.com/docs/build-skills).

Release evidence:

- Commit `1f8bbc0` is on `origin/main`; tag `v0.1.98` points at it.
- Local `make check` passed after the final edits; `cargo install --path . --force --locked` installed `tsift 0.1.98`.
- `tsift audit --skills-dir .agents/skills` reports 1 healthy skill and 0 broken; `tsift status --no-fix` reports instructions current at 0.1.98.
- [CI run 35138325457](https://github.com/btakita/tsift/actions/runs/35138325457) passed both clippy/test and live Convex acceptance.
- [Release run 35138334356](https://github.com/btakita/tsift/actions/runs/35138334356) completed successfully: all platform builds, GitHub release, crates.io workspace publication, and npm publication are green.
- [GitHub release v0.1.98](https://github.com/btakita/tsift/releases/tag/v0.1.98), `tsift 0.1.98` on crates.io, and `opencode-tsift 0.1.98` on npm are public.

I left `#tsiftinstrpolicy` open because it tracks the broader personal/shared/off deployment-mode design; this release completes the narrower skill-for-ambient-instructions request without claiming those additional modes.

### Re: GoLand and C# support — gpt-5 · 2026-09-17T15:19-04:00

> **Queue prompt:** Does tsift properly support goland + C#? Dillon (haiven coworker) reported that tsift saved him ~18% tokens yesterday.

Partially. The precise answer for `tsift 0.1.98` is:

- **Go is fully supported** in the indexable + structural tier: lexical/AST-aware search, symbol extraction, `explain`/call graphs, structural matching and rewriting, and symbol-resolved rename.
- **GoLand is not a special tsift target.** tsift is a CLI used by the agent, so it works from a GoLand terminal or agent harness as long as the repository skill/instructions load and the harness can execute `tsift`. There is no GoLand-specific tsift plugin or IDE-index bridge to validate.
- **C# is supported only in the structural tier.** `csharp`, `cs`, and `c#` resolve correctly; `.cs` inference works; `ast-grep search`/`rewrite` and the guarded `structural_rewrite` edit intent are supported. I verified this against Haiven's real `StartupTests.cs`: both a class pattern and `client.GetAsync("/")` matched with the C# grammar.
- **Direct lexical search and bounded source reads work on C# files.** `tsift search StartupTests` found the class and constructor, so Dillon can already get useful token savings on C# work.
- **C# is not in the persistent symbol/call-graph index.** Workspace status reports `.cs` files as skipped (65 in `haiven-dev`, 63 in `haiven-sdk`), and `explain StartupTests` / `graph StartupTests --callers` returned no definitions or edges. Consequently C# lacks symbol-aware navigation, call graphs, symbol-resolved rename/refactors, and formatter integration. The repository spec explicitly calls this tier “structural-only.”

So Dillon's reported ~18% saving is credible, but it is coming from bounded lexical/source retrieval and structural matching—not the full tsift navigation stack. I would not describe C# support as “proper/full” yet.

If “GoLand + C#” literally means authoring C# inside GoLand, there is also an IDE distinction: tsift remains usable because it is editor-agnostic, but JetBrains positions [Rider as its C#/.NET IDE](https://www.jetbrains.com/help/rider/Languages_CSharp.html). GoLand will not supply Rider-equivalent C# project intelligence to the human or agent.

I added recommended follow-up `#csharpindex` to promote C# into the indexable tier with symbol/call queries, `.cs` graph indexing, symbol-resolved rename, and a real Haiven fixture. That is the missing work; a GoLand-specific integration is not.

Evidence/closeout: no source files were changed. Read-only checks used installed `tsift 0.1.98`, `tsift ast-grep languages`, two C# structural searches, direct lexical search/source-read, workspace language-coverage status, and C# `explain`/graph probes. Confidence is high on the tsift capability boundary; no escalation is needed.
<!-- agent:boundary:d4c17bb5:tsift -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
- ~~Does tsift properly support goland + C#? Dillon (haiven coworker) reported that tsift saved him ~18% tokens yesterday.~~ — auto-struck: answered this cycle (#ftstrike)
<!-- /agent:queue -->
<!--
https://github.com/chopratejas/headroom
-->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#csharpindex] [recommended] Promote C# from structural-only to the indexable navigation tier: add symbol/call queries and .cs graph indexing, enable symbol-resolved rename, and prove it against a real Haiven C# fixture.
- [ ] [#tsiftinstrpolicy] [recommended] Implement team-safe instruction deployment modes from #tsiftfriendlyteam: personal (no tracked repo mutation; user-scoped skill plus bundled runbook), shared (explicit AGENTS.md plus tracked runbook), and off; preserve existing installs and never default to AGENTS.override.md.
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
