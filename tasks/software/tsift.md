---
agent_doc_session: tsift-v0.1
resume:
  claude: 4202ed71-8001-4378-b4cc-4f3974f43f7e
  codex: 01a0b337-c18d-7770-8e52-af605f601504
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

### Re: Response

> **Queue prompt:**
>
> do [#csharpindex]


> I meant golang, not goland. Please make C# fully supported.

Implemented `#csharpindex`: C# is now a fully indexed tsift language alongside Go. The root, graph, and search feature sets include `lang-csharp`; `.cs` files emit class, struct, interface, enum, record, delegate, method, local-function, property, and enum-member symbols plus direct/member/generic invocation call edges. C# is registered for indexed semantic edits, including identifier-aware `rename_symbol` that excludes comments, strings, same-named properties, and property reads when the selected symbol is callable.

Coverage now includes graph extraction/call-edge tests, cross-language conformance, rename safety, semantic-executor contract fixtures, and an end-to-end CLI test that indexes, searches, graphs, and renames a C# source file. I also corrected method call-edge ownership generally so calls inside indexed methods resolve to the method rather than the enclosing class.

Verification:
- `make check` passed.
- `cargo install --path .` passed and installed tsift 0.1.98.
- The installed binary indexed the real Haiven C# SDK fixture and resolved ten callers of `ServerFrame.Parse` across the conformance app, Unity edit/play-mode tests, and WebGL smoke code.
- Latest GitHub Actions run was green: `release: 0.1.98 -- repository skill integration`.

Commits pushed:
- tsift: `0e1d7e1 feat: add full C# indexing support` on `origin/feat/csharp-index`
- agent-loop: `4cac2b5b7 chore: point tsift at C# indexing support` on `origin/main`

### Re: team-safe instruction deployment modes — gpt-5 · 2026-09-17T17:29:14-04:00

> **Queue prompt:**
>
> do [#tsiftinstrpolicy]

Implemented team-safe instruction deployment modes and pushed them in `b77b430` (`feat: add instruction deployment modes`) to `origin/feat/csharp-index`.

- Added `tsift init --instructions auto|personal|shared|off` with persisted mode selection and compatibility detection for existing managed repository installs.
- Personal mode installs the user-level tsift skill/runbook and avoids tracked repository instruction changes; shared mode writes an explicit `AGENTS.md` router plus repository skill/runbook; off removes only tsift-managed repository instruction surfaces.
- Explicit Codex/OpenCode integration flags promote auto mode to shared and fail closed when combined with personal/off.
- Status now distinguishes disabled instruction scope and does not mistake an ambient global skill for initialized project instructions.
- Added CLI, index, status, and simulation coverage; updated README, specification, and unreleased notes.

Verification: `make check` passed (digest artifact `tart-a8ef871921`), `cargo install --path . --force --locked` succeeded, `git diff --check` passed, and the pushed branch is clean. No branch-specific GitHub Actions run was present immediately after the push.

### Re: release + publish — gpt-5 · 2026-09-17T23:42:22-04:00

> **Queue prompt:**
>
> release + publish

Released and published tsift `v0.1.99`.

- Release commit `ffc677e` contains full C# indexing support plus the team-safe `personal|shared|off` instruction deployment policy. It was pushed to `main` and tagged `v0.1.99`.
- Local release verification passed: `make check` (artifact `tart-29d81f4703`), `cargo install --path . --force --locked`, `tsift 0.1.99`, current generated instructions, and clean diff checks.
- The tag workflow passed release verification, built all three platform archives, published `opencode-tsift@0.1.99`, and published all 26 Rust crates at `0.1.99`.
- GitHub Release creation encountered `Error creating asset temp dir` because `dist/*` also matched staging directories. I recovered the release from the workflow artifacts and published all six archives/checksums at https://github.com/btakita/tsift/releases/tag/v0.1.99; every archive checksum verifies.
- Fixed that release regression in `8987a4c` by restricting uploads to archive/checksum globs and adding a regression assertion. The fix is pushed to `main` and `feat/csharp-index`; CI run `35303887336` passed both clippy/test and live Convex acceptance.
- `origin/main`, `origin/feat/csharp-index`, and the clean worktree now point to `8987a4c`; tag `v0.1.99` remains correctly pinned to release commit `ffc677e`.

Session note: the exchange is large, but the active queue is now drained without pausing for compaction.
❯ Fix discord.md issue:
```
❯ /home/brian/.cargo/bin/agent-doc start --route-owned --route-owned-reap-policy keep-alive --route-owned-start-purpose
 layout-provision --resume -- tasks/discord.md
Error: project controller command `start_session` failed: controller failed to start session actor for /home/brian/work/btakita/agent-loop/src/boost-client/tasks/discord.md: controller actor generation compare-and-swap failed for /home/brian/work/btakita/agent-loop/src/boost-client/tasks/discord.md: expected 2, found 3
```

### Re: Discord and monsterrodholders pane startup — gpt-5 · 2026-09-18T03:03-04:00

> **Queue prompt:**
>
> do [#fixdiscordissue]


Fixed, installed, and pushed as `e1ec2820e`.

- Route-owned startup now retries a generation CAS only when the newer authoritative actor still belongs to the same document, session, pane, and window. A genuinely different owner still fails closed.
- Cross-project editor reconciliation now falls back to the target project controller instead of requiring the currently executing controller request to own every nested project.
- `make check` passed completely: 9,499 Rust tests, editor parity suites, documentation audits, bootstrap tests, and model checks.
- The live install delivered the native reload to IDEA (`1/1` endpoint, `0 restart required`). No IDEA restart is needed for these fixes.
- Live verification recovered `monsterrodholders.md` in pane `%21` with a healthy supervisor and recovered `discord.md` in pane `%22`, ready with generation 6. The later route refusals were capability-proof checks after pane recovery, not pane-loading or generation-CAS failures.

The pre-existing unrelated JetBrains and session-actor worktree changes were left untouched.

### Re: tsift thrash — root cause and fix — opus-5 · 2026-09-21T04:58-04:00

> **Queue prompt:** tsift is currently thrashing. fix the tsift cause.

Fixed. One process, two compounding defects, both landed with mutation-checked regression tests.

#### What was thrashing

A single `tsift --envelope source-read` (pid 433178) had been running **11 hours at 97% CPU**, 2.7 GB RSS. Its command line was the tell:

```
tsift --envelope source-read .claude/projects/-home-brian-work-btakita-agent-loop-src-haiven-dev/memory/project_agent_doc_missing_replica_wedge_recovery.md --path /home/brian ...
```

`--path /home/brian` — it was indexing the entire home directory. Evidence from `/proc/433178`:

| signal | value |
|---|---|
| `rchar` | 101 TB |
| `syscr` | 24.7 billion read syscalls |
| `perf` top symbol | **`walFindFrame` — 89.36%** |
| `~/.tsift/index.db` | **4.0 KB** |
| `~/.tsift/index.db-wal` | **8093 MB** |
| tables committed | **zero** |

An 8 GB WAL in front of an empty database, after 11 hours. Nothing was ever committed.

#### Defect 1 — `$HOME` was resolving as a workspace root

`~/.tsift/` is tsift's *own* user-level state directory — `tsift-local-model` writes the GPU lease there (`$XDG_STATE_HOME/tsift/gpu-lease.json` → `~/.tsift/gpu-lease.json`), which creates the directory. From that moment `~/.tsift/` satisfied the `.tsift/`-directory workspace marker in both `project_root_from_canonical_path` and `harness_root_from_canonical_path`, so the ancestor walk from `~/.claude/projects/<slug>/memory/*.md` found no `.git` until it hit `$HOME` — and returned `$HOME`.

The code already knew about this bug class: it special-cased `$TMPDIR` (`ambient_temp_root`) and stopped there. `$HOME` is the same defect, and tsift is the one that creates the marker.

`ambient_state_roots()` now names `$TMPDIR`, `$HOME`, and `/`, and the `.tsift` marker is ignored at those paths. Scoped to that marker only — an explicit `.git`/`.gitmodules` repo at an ambient path (a dotfiles repo at `$HOME`) is user-created and still resolves as a workspace root.

#### Defect 2 — one unbounded transaction made the build quadratic

`apply_changes_inner` wrapped the *entire* apply in one `SAVEPOINT sp_apply`. `wal_autocheckpoint` (256 pages) only fires when a write transaction commits, so every dirty page stayed pinned in the WAL. Once the WAL outgrew its index hash blocks, SQLite's `walFindFrame` degraded into a backwards scan of those blocks on **every page read** — which is exactly the 89% and the 24.7 billion reads.

The apply now commits on a file boundary every 512 files (`INDEX_APPLY_COMMIT_CHUNK_FILES`), so the autocheckpoint can reclaim the WAL and the build stays linear in changed files. A change set that fits inside one chunk — the common incremental case — is still all-or-nothing. `file_state` is now written **last** per file, so the "fully indexed at this mtime" marker never precedes the rows it vouches for.

This one matters independently of defect 1: any sufficiently large repo would have hit it.

#### Proof the tests are not vacuous

Both were mutation-checked against pre-fix behavior:

- re-enable the `.tsift` marker at ambient roots → `ambient_tsift_state_dir_is_not_a_workspace_root` **FAILED**
- disable chunked commits → `apply_changes_commits_in_bounded_chunks` **FAILED**, `assertion left == right failed: expected the completed chunk to be durable, got 0 of 576`

Restored and touched after each mutation; both green again.

#### Verification

- `make check` → **exit 0**, 67 suites, **1850 passed, 0 failed** (counted from the runner's own `test result:` lines, not an output grep)
- End-to-end: the exact command that spawned the thrashing process now declines — `tsift rewrite: no rewrite: no project root found for source read ...` — so the original `cat` runs unchanged, with no `--path /home/brian`
- In-repo rewrites still resolve normally (`--path .../src/tsift`)
- `~/.tsift/index.db` did not reappear

#### Cleanup done

- Killed pid 433178; reclaimed the 8 GB orphan WAL (`index.db-wal`, `index.db-shm`, the empty `index.db`, and the stale `index.lock`). `artifacts/`, `prompt-cache-history/`, and `gpu-lease.json` were left alone.

#### Two things you should know

1. **The `src/tsift` checkout was 22 commits behind `origin/main`** (checkout at `0.1.88`, installed binary at `0.1.99`) with 25 dirty files. Those turned out to be a month-old `cargo fmt` pass plus generated `tsift init` output — no semantic content. I stashed them (`pre-thrash-fix: stale 2026-08-26 cargo-fmt pass + tsift init 0.1.99 regen`) and fast-forwarded before touching anything, so the fix is on current code. The stash is superseded by the 22 upstream commits; drop it unless you want it.
2. **`cargo install --path packages/tsift-cli` does not update the `tsift` binary** — that package's bin target is `tsift-cli`. The `tsift` binary comes from the workspace root crate, so `cargo install --path .` is the one that counts. It was previously installed from `.worktrees/tsift-csharp-index`; it now comes from `src/tsift`.

#### Landed

- `src/tsift` → `fcf26b2`, pushed to `origin/main`
- superproject pointer → `bfd53af95`, pushed
- spec: `specs/architecture.md` gained a rewritten **Transactional Index Updates** section and a new **Workspace Root Resolution** section
- CI run `35580603625` was still in progress at closeout; local evidence above stands on its own

Exchange is at 210 lines / 10 response sections — worth a compact when convenient.

### Re: #tsiftambientexplicit — refuse ambient state roots — opus-5 · 2026-09-21T05:35-04:00

> **Queue prompt:**
>
> do [#tsiftambientexplicit]


Decided and implemented: **refuse by default at the index-build boundary, and name the escape hatch rather than making it unreachable.**

#### The decision

`fcf26b2` stopped root *resolution* from picking `$HOME` off tsift's own `~/.tsift/` state directory. That closed the path the thrash actually took, but not the target. An explicit root still reached the index builder from an operator typo, `cd ~ && tsift index`, a hook that computed the root wrongly, or a future regression in resolution itself.

I argued both sides and landed on refusal because the escape hatch makes it cheap to be wrong in the safe direction. A whole-home index is now *bounded and linear* thanks to chunked commits, so it is no longer catastrophic — but a multi-GB index of every cache, checkout, and build directory the user owns is still not something anyone asked for, and I could not construct a legitimate use for it. `TSIFT_ALLOW_AMBIENT_ROOT=1` (also `true`/`yes`/`on`) overrides, so the one person who does want it is inconvenienced by a single env var instead of blocked.

#### Two guard sites, deliberately not the same check

The helpers moved from `tsift-quality::lint` down into a new **`tsift-index::roots`** module so resolution and the build guard share one definition (tsift-quality already depends on tsift-index, so no cycle).

| site | what it catches |
|---|---|
| `ensure_indexable_db_path` at `IndexDb::open` | Index dbs live at `<root>/.tsift/...`, so the root is the parent of the nearest `.tsift` ancestor. Refusing here means a refused run **creates nothing**. |
| `ensure_indexable_root` at `apply_changes_inner` + `rebuild_excluding` | Backstop for roots that never pass through a db path. In `rebuild` it runs **before** the bulk DELETEs, so a refused root never clears a healthy index on its way to failing. |

That first site came out of testing rather than design. My initial placement — the guard at `apply_changes` only — refused correctly but ran *after* `IndexDb::open` had already created `~/.tsift/index.db` (112 KB) and `index.lock`. The refusal was leaving behind a small version of the artifact it was rejecting. Verified fixed: `~/.tsift/` now holds only `artifacts/`, `prompt-cache-history/`, `gpu-lease.json`, and its lock after a refused run.

Scope is narrow on purpose: only the ambient paths **themselves** are refused. A directory *under* `$TMPDIR` or `$HOME` — where nearly every real project lives — indexes as usual, and an explicit `.git` repo at an ambient path still resolves as a workspace.

#### Verification

- Mutation-checked: neutering `ensure_indexable_root` fails **4 tests**, including both `IndexDb`-level integration tests (`apply_changes_refuses_an_ambient_state_root`, `rebuild_refuses_an_ambient_state_root_without_clearing_the_index`). All 12 pass restored.
- `make check` → **exit 0**, 67 suites, **1861 passed, 0 failed** (counted from the runner's own `test result:` lines).
- Live on the installed binary: `tsift index /home/brian` refuses with the message naming `TSIFT_ALLOW_AMBIENT_ROOT=1` and creates nothing; `tsift index .` in a repo is unaffected.
- **Last cycle's CI run `35580603625` completed green.** This cycle's run `35584042723` was in progress at closeout.

#### Landed

- `src/tsift` → `ebd2847`, pushed
- superproject pointer → `fa9b5849f`, pushed
- spec: `specs/architecture.md` — the **Workspace Root Resolution** section is now **Ambient State Roots**, covering both defenses and the escape hatch

Exchange is at 287 lines / 11 response sections; worth a compact when convenient.

### Re: release + publish — v0.1.100 tagged — opus-5 · 2026-09-21T05:49-04:00

> **Queue prompt:** release + publish

Released **v0.1.100** — version bumped, verified, committed, tagged, and pushed. The tag-driven workflow is running now.

#### What shipped

Both fixes from this session, plus their spec:

- ambient state roots (`$TMPDIR`, `$HOME`, `/`) no longer resolve as workspaces off a bare `.tsift/` marker, and are refused outright as index roots
- the index apply commits in bounded 512-file chunks so `wal_autocheckpoint` can reclaim the WAL

#### The bump

No automation exists for this, so it was mechanical: **129 occurrences across 47 files** — every crate `Cargo.toml` plus its inter-crate `version = "…"` dep refs, the `tsift:` version markers in `AGENTS.md` / `.agents/skills/tsift/` / `specs/search-navigation.md`, all 15 `opencode-tsift` command docs, `package.json`, and a fixture's `projection_version`. `Cargo.lock` regenerated via `cargo check --workspace`. I verified every dirty file's diff actually contained a version string before committing, so nothing unrelated rode along.

`VERSIONS.md` gained a `0.1.100` section under `Unreleased` with all three entries.

Verification before tagging: `make check` → **exit 0**, 67 suites, **1861 passed, 0 failed**. Installed binary reports `tsift 0.1.100`. Both prior CI runs (`35580603625`, `35584042723`) came back **green**.

#### One thing to watch on this run

**The previous release, v0.1.99, failed** — and the way it failed is worth knowing, because "failed" was misleading:

| job | v0.1.99 outcome |
|---|---|
| verify release inputs | ✓ 7m46s |
| build ×3 platforms | ✓ |
| **publish GitHub release** | **✗ 13s** |
| publish OpenCode npm plugin | ✓ |
| publish crates.io package | ✓ 15m52s |

crates.io and npm both **succeeded**; only the GitHub Release job died, with `##[error]Error creating asset temp dir`. Cause: `softprops/action-gh-release` was handed `files: dist/*`, which matched a staging **directory** as if it were an asset. Some `.sha256` and archive uploads still landed, so the v0.1.99 GitHub Release exists with an incomplete asset set. Confirmed independently: crates.io reports `max_version: 0.1.99`.

`8987a4c "fix: exclude staging dirs from release assets"` (2026-09-18) was landed for exactly this and **has never actually run a release** — v0.1.100 is its first exercise. So the one thing to check on this run is the `publish GitHub release` job specifically, not the top-line conclusion.

Run: [35585001349](https://github.com/btakita/tsift/actions/runs/35585001349) — still on `verify release inputs` at closeout, ~30 min end to end. I have a background poll on it and will report the per-job outcome.

#### Landed

- `src/tsift` → `dd25e02` (`chore: release v0.1.100`), tag **`v0.1.100`** pushed
- superproject pointer → `0f93ba244`, pushed

Two notes: last cycle's un-reaped `#tsiftambientexplicit` **self-healed** — this cycle's preflight persisted the reap and the backlog is now empty, so no repair was needed. And preflight's `semantic_completion_match` pointing at `#fixhttpsgithub` is a false positive; that was a different issue from 2026-08-27.

### Re: Does tsift support a skill so we don't need to use AGENTS.md? — opus-5 · 2026-09-22T13:22-04:00

> **Queue prompt:**
>
> do [#tsiftsupportskill]


> **Queue prompt:** Does tsift support a skill so we don't need to use AGENTS.md?

Partly, before this cycle. It does now.

Your Codex session on `src/haiven-dev/tasks/infra.md` answered "yes — use `tsift init --instructions personal`". That half is right: personal mode installs `~/.agents/skills/tsift/SKILL.md` and touches nothing tracked. But it stops one step short, and the step it skips is the one that matters.

#### The gap

**No harness scans `.agents/skills/`.** Claude Code reads `.claude/skills/`, Codex `.codex/skills/`, OpenCode `.opencode/skills/`, Grok `.grok/skills/`. The only thing making the canonical skill reachable was the marker-owned router `shared` mode writes into `AGENTS.md` — the exact file you want to stop using. So `--instructions personal` installed a skill *nothing loaded*.

Your own machine is the proof:

| path | version | loaded by Claude Code? |
|---|---|---|
| `~/.agents/skills/tsift/SKILL.md` | **0.1.100** (generated) | **no** |
| `~/.claude/skills/tsift/SKILL.md` | **0.1.62** (hand-written) | yes |

The skill in this session's own skill list is the 0.1.62 one — 38 versions stale — while the current generated skill sits in a directory nothing reads. `~/.codex/skills/` holds `compose-skills` and `watch`, not tsift, so Codex was not loading it either.

#### What I built

`tsift init --harness <claude|codex|opencode|grok|all>` — the four you named — symlinks the canonical skill into each harness's own skill directory.

```bash
tsift init --instructions personal --harness all   # what you want for haiven-dev
```

```
~/.claude/skills/tsift   -> ~/.agents/skills/tsift
~/.codex/skills/tsift    -> ~/.agents/skills/tsift
~/.opencode/skills/tsift -> ~/.agents/skills/tsift
~/.grok/skills/tsift     -> ~/.agents/skills/tsift
```

Verified end to end in a scratch repo: all four resolve to `tsift:skill v=0.1.100`, and the repo stays clean — no `AGENTS.md`, no tracked diff.

**A symlink, never a copy.** A copy is exactly how you ended up with a 0.1.62 skill next to a 0.1.100 one: the harness keeps loading a release-old file while `tsift status` reports the canonical surface as *current*, because status only ever reads the canonical path. One symlink means one `tsift init` refresh updates every harness at once, and staleness stays detectable.

Other decisions worth knowing:

- **Shared mode links relatively** (`../../.agents/skills/tsift`), so the checkout stays relocatable — I moved a test checkout and the link still resolved. Personal links are absolute because their target is outside any project. The `..` climb is derived from each harness directory's own depth, so a harness added at a different depth can't silently produce a dangling link.
- **An existing path that isn't a tsift-owned link fails closed**, naming the harness and path. Your hand-written `~/.claude/skills/tsift/` will hit this — see below.
- **`--instructions off --harness all` removes the links** from both project and user roots, so tearing down the canonical skill can't strand a dangling skill directory.
- **`--harness` does not require `shared`**, unlike `--codex`/`--opencode`. Making the skill loadable without tracked files is the entire point of personal mode.

#### One thing that will stop you

`--harness claude` **will refuse** on your machine, by design:

```
Refusing to replace the unmanaged claude skill at ~/.claude/skills/tsift — move it aside, then re-run with --harness claude
```

That file is hand-maintained (7.6K, `user-invocable: true`, `/tsift` trigger, its own `runbooks/`) and richer than the generated 2.4K skill, so destroying it silently would be the wrong default. And since `--harness all` bails on the first refusal, nothing gets linked until you resolve it. Two options: move it aside and let the link take over, or run `--harness codex --harness opencode --harness grok` and keep managing Claude's by hand. Backlogged as `#claudeskillmerge` since the real fix is folding its extra content into the generated template.

#### Verification

- `make check` → **exit 0**, 67 suites, **1868 passed, 0 failed** (7 new tests).
- Mutation-checked both guards rather than trusting them: breaking the relative-target climb reddened 3 tests; forcing `is_tsift_owned_skill_link` to always return true reddened the refusal test. Restored, re-verified green.
- `cargo install --path .` → `tsift 0.1.100` with the new flag live.
- Probes: personal + shared modes, all four harnesses resolving, relocatability after a checkout move, idempotence (`already present`), and refusal against an unmanaged skill.

#### Landed

- `src/tsift` → `aac7c11`, pushed
- superproject pointer → `e342169f04`, pushed
- CI [35760196615](https://github.com/btakita/tsift/actions/runs/35760196615) — **still in progress** at closeout, reporting from local evidence.

One correction to last cycle's record: it said both prior CI runs were green. The v0.1.100 release run `35584999376` actually **failed** — `search_cmd_uses_wal_snapshot_fallback_when_lock_appears_after_precheck`, 479 passed / 1 failed, CI-only. That test installs a process-global post-precheck WAL-lock hook, so a parallel test sharing that global is the likely mechanism. Backlogged as `#ciwalflake` rather than waved off as flaky.

### Re: #claudeskillmerge — what the evals say, and versions now tested — opus-5 · 2026-09-22T13:34-04:00

> **Queue prompt:** do [#claudeskillmerge]: What do the evals say? Add evals to test the versions.

#### What the evals say: nothing — and that is the finding

`evals/` holds `agent-doc/{agentic,diff-interpretation,write-latency}` and `general/token-efficiency`. **There is no tsift suite at all**, so the evals had nothing to say about skill versions.

The sharper answer is that the deterministic check was already there and was passing on a broken input. `tsift audit` exists to detect skill drift. Pointed at the real skills directory it reported:

```json
{"name": "tsift", "has_skill_md": true, "description": "Primary source for the tsift code-RAG CLI…", "issues": []}
```

`total=3 healthy=3 broken=0`. It looked straight at a skill pinned to `0.1.62` next to a `0.1.100` binary and called it healthy — because a stale skill has a `SKILL.md` and it has a `description`, which is all the audit ever checked. That is the eval to add, and it belongs in `audit` rather than in an LLM rubric: version equality is a deterministic property, and a judged eval would be a worse instrument for it.

#### `#skillversioneval` — the audit now reads the declared version

The scan reads the version a skill *declares*, from either surface tsift stamps:

1. `<!-- tsift:skill v=0.1.100 -->` — the ownership marker on a generated skill
2. `tsift-version:` frontmatter — how a hand-maintained tsift skill pins what it was written against

Marker wins when both are present. A mismatch becomes an issue and moves the skill from `healthy` to `broken`; the value surfaces as `declared_tsift_version` in JSON. A skill declaring neither is not tsift's and is **never** flagged — inventing a version for an unrelated skill would manufacture drift across the whole directory. The expected version is the running binary's own `CARGO_PKG_VERSION`, not a separately maintained constant that could drift on its own (there is a test pinning exactly that).

Same command, same directory, after install:

```
total=3 healthy=2 broken=1
 ISSUE tsift | declared=0.1.62 | tsift version drift: SKILL.md declares 0.1.62
              but the installed tsift is 0.1.100 — run `tsift init` to refresh it
```

#### `#claudeskillmerge` — decision: fold it in

I folded the hand-maintained skill's durable value into the generated template, so the symlink can take over. The reason to fold rather than keep two: **the private copy is the one that went stale.** Operators kept it because the generated skill was thinner, and thinness is what created the divergence.

Folded in — `user-invocable: true` + `argument-hint` (so `/tsift` works as a slash command, which the generated skill had lost), `TRIGGER:`/`SKIP:`/`VERSION CHECK:` in the description, a `tsift-version` stamp, a version-check paragraph, a compact command surface, and a line telling harnesses without a `PreToolUse` rewrite to drive the envelope commands themselves.

Deliberately not folded — the `../../../src/tsift/SPEC.md` relative links (they only resolve from one checkout), the five extra runbooks (`references/code-navigation.md` already covers that ground and `tsift --help` is the live surface), and **`tsift status --fix`**, which the hand-written skill taught twice and which is in `DEPRECATED_FLAG_USAGES`. A gate test now asserts the enriched template still passes that check.

An existing gate caught me mid-edit and was right: `SKILL.md` must not restate the reference's commands, and must stay the smaller of the two. My first draft made the skill 4402 bytes against a 4363-byte reference. Rather than shave bytes I put the full command surface and the version-drift detail into the reference — which had **no** command surface section at all, a real gap the fold exposed. Skill 4402, reference 6233.

#### Verification

- `make check` → **exit 0**, 67 suites, **1875 passed, 0 failed** (7 new tests this cycle).
- Mutation-checked the new guard rather than trusting it: neutering the version comparison reddened both drift tests; restored and re-verified green.
- Live `tsift audit` before/after against the real skills directory, shown above.
- `tsift init` re-run on this repo, restamping both generated surfaces; `tsift status` → `instructions: current (v0.1.100)`.

#### Landed

- `src/tsift` → `b4bedd1`, superproject → `ea2080abc7`, both pushed.
- CI [35761622442](https://github.com/btakita/tsift/actions/runs/35761622442) queued at closeout.
- **Last cycle's CI run `35760196615` came back green** — the `--harness` work passed.

`~/.claude/skills/tsift/` is still the hand-written 0.1.62 file; that is yours to move aside, and `tsift audit` will now tell you so every time you run it. Once it is gone, `tsift init --instructions personal --harness all` links the generated skill into all four harnesses.

Exchange is at 453 lines / 14 sections; noting it rather than pausing the drain. Next head is `#ciwalflake`.
<!-- agent:boundary:d4c17bb5:tsift -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
- ~~do [#claudeskillmerge]: What do the evals say? Add evals to test the versions.~~ — auto-struck: answered this cycle (#ftstrike)
- do [#ciwalflake]
<!-- /agent:queue -->
<!--
https://github.com/chopratejas/headroom
-->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#ciwalflake] Fix the CI-only failure in tsift-cli test search_cmd_uses_wal_snapshot_fallback_when_lock_appears_after_precheck — run 35584999376 (the v0.1.100 release commit) failed there with 479 passed / 1 failed while the full local suite is green. The test installs a process-global post-precheck WAL-lock hook, so a parallel test sharing that global is the likely mechanism.
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
