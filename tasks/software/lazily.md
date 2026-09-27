---
agent_doc_session: d818612d-3614-4c43-b5ec-83923406aa46
resume:
  claude: d6379dbb-42f5-4658-8da6-e0251c5eaa18
  codex: 01a0da83-7762-7810-8ddf-49eda4e9a807
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
agent_doc_model_tier: high
prompt_presets:
  '#actionable': Add actionable tasks into backlog + queue
  '#actionable-review': Add actionable review items into backlog + queue
queue: go
---

## Status

<!-- agent:status patch=replace -->

completed
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/44ae7f3a5c03b112ffe17aa5c85d000e3d79a6e9a4875e16442309ebf1c06a1c-20260926-171904.md`*

Compacted content:
- Archived 37 response topic(s): Archived 11 response topics; #actionable-review; Codex attribution and queued work; 34 more
- Prior summary/context: prior compacted content: Archived 11 response topic(s): Lazily stdlib matrix release; Is lazily-rs published; Feature...

### Re: durable runtime admission closeout — gpt-5 · 2026-09-26T13:39-04:00

> **Queue prompt:**
>
> do [#lzdurableadmissioncloseout]


Completed `#lzdurableadmissioncloseout`.

- Added `src/lazily-rs/docs/dependency-license-inventory.json`: a deterministic all-features `Cargo.lock` inventory of 415 packages and 28 reviewed license declarations, including exact versions, enabled features, runtime/build/dev reachability, selected SPDX branches, source URLs/checksums, license/notice evidence, and notice disposition.
- Added the fail-closed `scripts/dependency-inventory.py` gate plus `make dependency-inventory-check`; `make check` and the named CI step now enforce inventory, license/source/provenance, and durable-source-digest drift. The CI-reach closure and exact step map prove the new gate is active.
- Added `docs/durable-runtime-provenance.md`, naming the official PostgreSQL, JetStream, rust-postgres, and nats.rs sources, independent authorship, synthetic fixtures, selected `postgres 0.19.14` / `async-nats 0.50.0`, and generic exclusion of employer/customer inputs. Public-repo private-name hygiene passed.
- Updated `tasks/software/plan-lazily-postgres-nats-valkey.md` truthfully: the selected clients and closed public license/provenance gate are recorded; the wider family license reconciliation and private employer-boundary attestation remain open. This private record confirms no Haiven or other employer inputs, time, equipment, credentials, or accounts were used, without claiming or creating the attestation.
- Verification: `make dependency-inventory-check` passed; the negative regeneration control refused `--write` without explicit review acceptance; `make ci-reach` passed; the full `make check` passed with exit 0 and zero detected failures. The new GitHub CI run is in progress, so it is recorded as pending rather than used as closeout evidence.
- Commits: `lazily-rs` `74af6c9` (`chore: enforce dependency license admission`) was pushed to `origin/main`; private planning record `c17eb79c82` (`docs: close durable runtime admission gate`) is committed in the superproject.

The JetBrains advisory remains: run the plugin installer once to replace the superseded live jar bytes; restart the IDE only if that install reports a dynamic-unload failure.

### Re: root instruction-files target paths — gpt-5 · 2026-09-26T13:52-04:00

> **Queue prompt:**
>
> do [#lzrootinstructionpath]


Completed `#lzrootinstructionpath`.

- Corrected the root `Makefile` `publish` and `clean` recipes from `cd instruction-files` to `cd src/instruction-files`.
- Verified both targets without executing them: `make -n publish` and `make -n clean` resolve the corrected path, `src/instruction-files` exists, and the dry-run output contains no stale `cd instruction-files` form. No publisher or cleaner ran.
- Refreshed `tasks/software/plan-ambient-publish-token-point-of-use.md` with the corrected root credential hunk and the new Makefile preimage SHA-256 `c574a714867cdf8a277714d4173a059f91f1341a6e5dad19ff78d2e1c167d891`.
- Reproved the plan: all eight recorded preimage hashes match; all seven fenced patch sets pass `git apply --unidiff-zero --check`; the exact local publisher inventory still matches the recorded consumers plus the tagpath dry-run/prose-only check; and every workflow-owned publisher file carries a repository-secret or OIDC authentication marker. No credential reference was evaluated.
- Verification: the complete root `make check` passed with exit 0 and zero detected failures. This superproject has no workflow named `CI`, so no corresponding remote CI run exists to inspect.
- Committed as `2c3ed5fdcc` (`fix: correct instruction-files target paths`). The credential-scoping patches remain unapplied, no real publisher was invoked, and no credential was read, rotated, or changed.

### Re: actionable review extraction — opus-5 · 2026-09-26T14:16-04:00

> **Queue prompt:**
>
> #actionable-review

Audited all four gated review items against live state. Two were hiding agent-executable work inside an operator gate; two are correctly gated and stay that way.

**Extracted into backlog + queue** (appended, so they do not preempt the current drain):

- `#lzfakepublisherproof`: the plan's fake-publisher proof does not exist anywhere. A search across `*.py`, `*.sh`, `*.rs`, and `Makefile` in the whole superproject found no harness, so the "fake-publisher and fresh-shell verification" blocker on `#lzambienttokenresolve` is agent-buildable today rather than operator-gated. The spec is already written at `tasks/software/plan-ambient-publish-token-point-of-use.md` lines 228-280.
- `#lzartifactpurgeexec`: `src/agent-doc/scripts/audit-actions-artifacts.py` is already parameterized (`--repo`, `--output-dir`), so re-intersecting candidates against the live API is a re-run, not new work. What is missing is the fail-closed executor. The handoff README states "no deletion command is included," and the 466-row withheld-artifact invariant lives only in prose. Building a dry-run-default executor that refuses withheld ids deletes nothing and leaves the destructive run under `#lzagentdocartifactpurge`.
- `#lzdocorphanresidue` (`[operator-verify]`): three truncated fragment lines sit after `&lt;!-- /agent:done --&gt;` in this document, beginning `eturn typed satisfied/`, `cate orphan line remains outside`, and `ebc036`. They are residue from a prior compaction, outside every agent-doc component, so component writes cannot reach them, and a disk edit under the live JetBrains editor would fight the editor's authority. Tagged for operator deletion so it does not churn the drain.

**Stays gated, facts re-verified rather than trusted:**

- `#lzmacosrunner`: `make release-macos-cadence-check` reports the weekly window open since 2026-09-22T16:41:49Z (v0.35.398 uploaded Darwin assets 2026-09-15), and `gh release view` confirms v0.35.417 is still `Latest` carrying six assets with no Darwin archive. The gate is real, because Darwin archives need a Mac. Text refreshed with the verification date.
- `#lzgithubbudget20`: blocked on a payment method on the personal GitHub account, with no agent-executable slice. Left unchanged.

**Corrected stale review text:** `#lzambienttokenresolve` asserted that "agent-executable prerequisite #lzrootinstructionpath is now tracked in backlog and queue." That item completed this session as commit `2c3ed5fdcc` and the backlog is empty, so the sentence was stale. It now names `#lzfakepublisherproof` as the remaining agent-executable prerequisite.

I filed these two rather than implementing them because the queued instruction was the extraction itself. Both are now drainable heads, so the next cycle can take them.

**JetBrains advisory persists** (`stale_plugin`, `plugin_bytes_superseded`): live pid 3129637 has `agent-doc-jetbrains-0.2.419.jar` mapped with its inode unlinked, which no version string can reveal. Run the plugin installer once so its dynamic update transaction converges, and restart the IDE only if that install reports a dynamic-unload failure. This document does not own agent-doc development, so I did not install.

### Re: queue halted by operator — opus-5 · 2026-09-26T14:24-04:00

Acknowledged `queue: stop`. Preflight resolved `queue_active: false` with `queue_drainable_head_count: 0`, so I drained nothing and executed no repo work. The three heads and their backlog items are left intact, so re-enabling `go` resumes exactly where this stopped; I did not strike them, because discarding queued intent is your call, not mine.

Why each stays open, since none was executed:

- `#lzfakepublisherproof`: not started. Still the agent-executable prerequisite for `#lzambienttokenresolve`.
- `#lzartifactpurgeexec`: not started. Still non-destructive work that deletes nothing.
- `#lzdocorphanresidue`: `[operator-verify]` by construction, and it needs your deletion in IDEA regardless of queue state.

**`agent-doc plan` contradicted preflight on this turn, and the contradiction is a false-closeout hazard.** With the queue inactive, `plan` still emitted `repo_actions` for all three heads and a `required_commands` line of `agent-doc finalize ... --done lzfakepublisherproof --done lzartifactpurgeexec --done lzdocorphanresidue`. Following that literally would have marked three unexecuted items complete, including an `[operator-verify]` item that no agent turn can satisfy. `plan` appears to derive `repo_actions` from queue prose without consulting the `queue_active` state that preflight already resolved. Filed to `tasks/agent-doc/agent-doc-bugs2.md` as `#planinactivequeuedone`; I trusted preflight, per the rule that drainability is never re-derived from queue prose.

**New in this cycle's advisories:** the running binary is missing one committed change, `df02ce27c fix: route pane focus and stop by document actor`. That is a stop-routing fix, on a turn whose entire subject is a stop, so treat any stop/focus-routing oddity you see right now as possibly pre-fix behavior rather than a new defect. I did not run `make install`, since this document does not own agent-doc development.

The two operator actions from the previous cycle are still outstanding: re-run the JetBrains plugin installer once (live pid 3129637 still has an unlinked jar inode mapped, which is the likely cause of this session's retained-write deferrals), and delete the three orphan fragment lines in this document.

### Re: fail-closed artifact purge executor — opus-5 · 2026-09-26T15:18-04:00

`#lzartifactpurgeexec` is done and pushed as `187f31622` (`feat: enforce artifact purge refusals in code`). The two invariants that lived only in the handoff README are now code.

**`src/agent-doc/scripts/purge-actions-artifacts.py`** is the only command that may call the artifact deletion endpoint. Dry run is the default; deletion needs `--execute` **and** `--authorize-deletion <owner/repo>` whose value equals the audited repository, and that pair is checked before any network call.

Candidacy is re-derived live rather than trusted from the CSV. The executor imports the audit generator and calls its own `counterpart()` against a fresh enumeration of the Actions artifacts, Releases, and PyPI files, so candidacy has exactly one definition in the repo. An id is refused per-artifact when the live artifact is renamed, resized, has a different expiry, has no durable counterpart any more, or whose counterpart kind/name/URL drifted from the audit row. An id gone from the live API or already expired is *skipped*, never reported as deleted.

Withheld rows are refused twice over: an id in `artifact-unresolved.csv` can never enter the delete set, and naming one with `--only` aborts the run outright. A candidate/withheld overlap aborts at load — if the two CSVs disagree, no row in either is trustworthy.

Structural faults all abort before the network: a candidate with no counterpart URL, summary totals that disagree with the CSV rows, a summary already claiming `deletion_performed`, a handoff older than `--max-handoff-age-days` (7), a `--repo` that does not match the audit, or a PyPI project the audit module cannot use to verify that repository. A plan above `--max-deletions` (250) aborts rather than deleting a truncated subset.

**Coverage.** `--self-test` exercises every refusal path offline against a fake client — unauthorized `--execute`, wrong-repo authorization, `--only` on a withheld id, `--only` on a non-candidate, size drift, rename drift, vanished release asset, untagged run, absent id, expired id, the deletion cap, CSV overlap, missing counterpart URL, both summary-total mismatches, a summary claiming a prior deletion, a stale handoff, missing files, re-derivation scope, plus end-to-end CLI runs proving dry run never reaches the deleter and an authorized run deletes exactly the planned ids. A non-204 status and a raised exception both report failure instead of success. Two assertions make the test refuse to touch the network at all: an injected client factory that raises if a refusal was supposed to fire earlier, and a deleter that raises if anything tries to delete. `make check` runs it via the new `artifact-purge-check` target.

**Verification.** `make check` passed with exit 0, 9,770/9,770 tests, Lean and TLC green. The first run failed on `plan::tests::finalize_placeholder_keeps_small_done_flag_lists` with `No such file or directory` execing the test binary — a concurrent relink of `agent_doc-8c52c9c9410fc3e0` in this shared tree, not an assertion; the binary was present afterward and the rerun was clean.

The live default dry run against `btakita/agent-doc` planned **2,771 deletable artifacts (45.47 GB)**, skipped **10** now absent from the API, refused **0**, and deleted nothing. Zero refusals means the 2026-09-25 rows still match live metadata exactly, so the audit has not decayed in a day. The default 250 cap correctly aborted that 2,771-artifact plan on the first attempt — deleting the full set is a deliberate `--max-deletions 3000`, which is the fail-closed behavior I wanted from a cap.

I also updated the generator's `report_markdown` and the committed handoff README with a `Purging` section, taking the README text straight from the generator so a regeneration cannot drift from it.

`#lzagentdocartifactpurge` keeps its review gate and now names the exact command plus the authorization flags; nothing was deleted, and no code path deletes without both explicit flags. CI run `36265576676` for this commit is in progress — not waited on, per the observe-don't-await rule.

Remaining drainable head is `#lzfakepublisherproof`. `#lzdocorphanresidue` still needs your three-line deletion in IDEA.

### Re: fake-publisher proof harness — opus-5 · 2026-09-26T15:52-04:00

> **Queue prompt:**
>
> do [#lzfakepublisherproof]


`#lzfakepublisherproof` is done and pushed as `cf2cbfda9` plus a follow-up `3e48cef93`. The plan's five-step manual proof is now `make publish-credential-proof`, wired into root `make check`.

**It runs before application, which is the whole point.** The patches are still unapplied and operator-gated, so the harness rebuilds each *patched* file in a scratch git checkout, extracts the patched recipe lines, and runs exactly those. It also proves the extraction is honest: a hunk's added recipe lines must appear *contiguously* inside the target's recipe in the patched file, or the run fails as drifted hunk offsets.

Coverage is all eight production call sites — root instruction-files, module-harness, corky's six crates, corky maturin, agent-doc's venv twine, lazily-py twine, ctx-core-dev bun, plus unpatched TestPyPI — in success, failure, and empty modes: **22 checks, all green**. Success asserts exactly one `passage` lookup for the expected key, that the publisher actually ran, and that its child saw *exactly* the point-of-use variable carrying the sentinel — no root alias, no `NPM_CONFIG_TOKEN`, nothing credential-shaped on the command line. Failure and empty assert a nonzero exit with **no publisher invoked at all**. Measured, not claimed: 21 fake `passage` lookups (7 patched sites × 3 modes) and 13 fake publisher invocations (success mode, corky contributing 6). Twine is intercepted as the plan specifies — a fake `twine/__main__.py` on a temporary `PYTHONPATH`, since a `PATH` executable does not intercept `python -m twine` — and agent-doc's absolute venv interpreter gets a temporary wrapper.

No credential is resolved, printed, or rotated, and no real publisher runs. Every value a recorder observes is masked before it reaches a log; only the sentinel and an explicit nonsecret allowlist (`__token__`) are written verbatim. I scanned the report for token-shaped strings: the only long strings are the plan's own SHA-256 preimages.

**Three things I had to correct rather than assert.**

First, my fresh-shell phase was measuring nothing. A non-interactive `zsh -l -c` does not fire mise's `precmd` activation hook, so it saw zero aliases and would have "passed" vacuously — and would keep passing after application for the wrong reason. Forcing activation means sourcing `.env`, which resolves real credentials, so I will not do it. The assertion now rests on the root `.env`'s alias **name** state, read without ever evaluating the file: 6 assigned, 0 commented, exactly the unapplied state. The plan's literal step-5 loop still runs, but is labelled `inconclusive` with its reason instead of counted as a pass. Rerunning it interactively after application is now an explicit operator step on `#lzambienttokenresolve`.

Second, this machine's environment carries none of the aliases, so the parent-env scrub is a no-op here and the child-side assertion was structurally sound but never contrastively demonstrated. Two controls are now end-to-end with an obvious decoy injected past the scrubber: the pre-patch recipe inheriting an ambient `CARGO_REGISTRY_TOKEN` must be rejected, and an unrelated ambient `NPM_TOKEN` reaching the publisher child must be rejected. `parent_env_names_removed` reports when the scrub was a no-op.

Third, I had written `real_publisher_invoked: false` into the report. That was an unmeasured claim dressed as an observation; it is gone, replaced by the lookup and invocation counts above, which a failed interception would drive to zero and redden.

`--self-test` runs all of those negative controls *before* the real proof, so a proof whose assertions cannot fail never reports success.

**The harness immediately earned its keep:** `src/agent-doc/Makefile`'s recorded preimage no longer matched, because my own previous commit `187f31622` added `artifact-purge-check` to that file. I refreshed that hash and its hunk offset (327 → 334), and the root `Makefile` hash this change drifts. All eight preimages match and all eight patches apply again.

The follow-up commit fixes a real consequence of wiring this into root `make check`: the superproject's `Agent Doc` workflow runs that target, and its checkout has no `.env` and initializes only seven submodules, so five of these repos are absent or present as empty directories. Absence is now classified — `repo_absent` for an uninitialized submodule, `ignored_file_absent` for a missing `.env`, and a hard failure only when a *checked-out* repo lacks the file. Skipped entries are listed explicitly so reduced coverage is visible, and a run that exercises no call site at all fails outright so the skip path cannot become a vacuous pass. A simulated CI checkout exits 0 reporting 2 exercised / 6 skipped; a root holding only the plan exits 1 on that guard.

**Verification.** Root `make check` exit 0, 9,770/9,770 agent-doc tests, proof and self-test green inside it. An intermediate root run went red on agent-doc's `plugin-version-check` (JetBrains packaging fence) — that was a concurrent session's mid-edit JB sources in this shared tree, not my change; those paths are clean and the gate passes now, and I re-ran root check to exit 0 rather than report the stale red. Both superproject workflows are `workflow_dispatch`-only, so this push triggers no CI run; the three agent-doc CI runs, including last cycle's `187f31622`, are all still in progress and not waited on. Exchange is at 107 lines with the queue active — noting the size, not pausing to compact.

Remaining queue head is `#lzdocorphanresidue`, which is `[operator-verify]`: the three truncated orphan lines after the `agent:done` close marker still need your deletion in IDEA.

### Re: what #lzdocorphanresidue needs — opus-5 · 2026-09-27T00:20-04:00

> **Queue prompt:**
>
> [#lzdocorphanresidue]: What needs to be done here?

**Delete lines 203-205 of this file in IDEA, save, and send `continue`.** That is the whole item.

They are the last three lines, immediately after the `&lt;!-- /agent:done --&gt;` close marker at line 202:

| line | content (truncated) |
|---|---|
| 203 | `eturn typed satisfied/timed-out/cancelled/disposed-or-unavailable outcomes; optionally prove keyed side-effect receipts…` |
| 204 | `cate orphan line remains outside all agent-doc components at line 158. Please delete only the stray `- [ ] [#lzstdlibbarrier] ...` line…` |
| 205 | `ebc036` |

Each begins mid-word (`eturn`, `cate`, `ebc036`) because they are truncated tails left behind by an earlier compaction — the head of each line was archived and the remainder was stranded. Line 204 is itself the orphaned tail of a *previous* instance of this same problem, which is how the residue accumulated. Nothing references them; deleting all three is lossless.

**Why I cannot do it for you.** Two independent reasons, both verified this cycle rather than assumed:

1. The lines sit *after* `&lt;!-- /agent:done --&gt;`, outside every `agent:*` component. Every write path the agent owns — `agent-doc write`, `patch`, the backlog/review/done flags — addresses a named component, so none of them can reach text in that gap.
2. A live JetBrains editor holds the document and owns authority: the ops log reports `realtime_doc_resolve authority=editor_buffer live_editors=1 delivery_converged=true` and `crdt_current_text authority=multi_replica live_editors=1 delivery_version=15`. Editing the file on disk under that would be hand-aligning disk to the authority, which is the one move the recovery rules forbid outright — it manufactures a divergence the editor then has to arbitrate.

Disk and the editor buffer currently agree exactly (both tails identical, `text_len=25457`), so there is no divergence to repair — only these three lines to remove.

**If you would rather not touch it by hand:** close the `lazily.md` tab in IDEA. That drops `live_editors` to 0, disk becomes the authority, and I can clear the lines and strike the item on the next invocation. Tell me once the tab is closed.

The item stays open and `[operator-verify]` either way — this turn answered your question rather than executing the deletion, so there is nothing to mark done yet.

One note while I was in here: preflight reports `queue_maintenance_not_persisted` (`pending add: custom id already exists: lzdocorphanresidue`). The queue add was a no-op because the id was already queued, so the queue text you see is current — no action needed.

### Re: orphan residue cleared — opus-5 · 2026-09-27T00:39-04:00

> **Queue prompt:**
>
> do [#lzdocorphanresidue]

Closing `#lzdocorphanresidue` — you deleted the three lines, and I verified the removal rather than taking the queue re-add as proof.

- **Disk:** the byte range after `&lt;!-- /agent:done --&gt;` is now empty; the close marker is the last content in the file. Non-blank line count in that gap: `0` (was `3`).
- **Editor authority:** `agent-doc read` returns the same — `0` non-blank lines past the close marker. Disk and the CRDT replica agree, so there is no disk-change candidate left for the editor to arbitrate.
- The preflight diff independently confirms it, attributing all three removals to you: `[user-] eturn typed satisfied/…`, `[user-] cate orphan line remains outside…`, `[user-] ebc036`.

Every `agent:*` component is now the full extent of the document, which is what the item asked for. Nothing was archived or lost — those lines were truncated tails whose heads had already been archived by an earlier compaction.

Marked `--done`. The queue is empty and the four review items remain gated: `#lzmacosrunner` (needs Darwin hardware), `#lzgithubbudget20` (needs a payment method on the personal GitHub account), plus `#lzambienttokenresolve` and `#lzagentdocartifactpurge`.

Two things worth noting from this stretch, neither of them blockers:

- The exchange is at 171 lines across 7 response sections, past the 160-line warn threshold. No `agent_doc_auto_compact` opt-in is set, so I am not compacting — flagging it since compaction residue is exactly what this item was cleaning up.
- `agent-doc doctor` reported `no issues detected` on this document the whole time those three orphan lines were present, and `agent-doc autofix` had no catalog entry for the class. Text stranded outside every component is undetected by both, which is why this needed a human to notice it. If you want that detector built, queue it and I will take it.

### Re: advancing the four gated review items — opus-5 · 2026-09-27T01:05-04:00

> **Queue prompt:**
>
> #advance-review

Re-verified all four against live state rather than trusting their text, and turned one of them into code. Two items moved materially; two are still operator-only, and one of those I could not verify at all — I say so instead of re-asserting it.

**`#lzmacosrunner` — the policy's second half is now enforced, not prose.** The cadence gate only ever bounded how *often* Darwin assets may be uploaded. Nothing asserted that any release actually *carries* them, so "never drop either deliverable" had no enforcement — and the deliverable has been dropped five times since v0.35.398 with `make release-macos-cadence-check` still reporting success. New `make release-macos-coverage-check` (commit `ddb612833`, pushed) names them exactly: **v0.35.399, v0.35.400, v0.35.401, v0.35.406, v0.35.417**. A half-shipped pair counts as a drop; drafts are excluded; a published release with no timestamp fails closed rather than being skipped, so an unorderable release cannot hide a dropped deliverable. It is deliberately **not** a `make check` dependency — clearing it needs Mac hardware, and blocking the build on hardware would redden every unrelated change. `specs/07-core-commands.md` records why the cadence gate is blind to this class, and `test_cli` pins both the gate's existence and that exclusion.

**`#lzagentdocartifactpurge` — the audit decayed in one day, and the fail-closed executor caught it.** Yesterday's live dry run refused 0. Today it refuses **105**, all `no_live_durable_counterpart`, and the plan shrank from 2,771 to **2,666 artifacts (43.83 GB)**, 10 skipped, 0 deleted. Cause, verified directly against PyPI rather than inferred: **PyPI's oldest surviving `agent-doc` release is now 0.35.113**, so 27 versions in 0.35.0–0.35.37 lost their wheels. For those artifacts the Actions copy is now the only copy, and the executor's live re-derivation refused every one. That is the design working — the refusal is the feature, and it is why this run is still safe to authorize.

Two facts the operator's authorization now depends on, neither previously recorded:

- **20% of the remaining plan rests on a store that demonstrably prunes.** 530 of the 2,666 candidates are backed by PyPI files alone, lowest surviving backer 0.35.119 — six versions above the prune floor. The other 2,136 rest on GitHub release assets, which have not decayed.
- **There is a hard deadline.** The handoff was generated 2026-09-25T17:02:35Z and the executor's 7-day `--max-handoff-age-days` guard aborts the entire run after **2026-10-02T17:02:35Z**, unless the audit is regenerated or `--allow-stale-handoff` is passed.

**`#lzambienttokenresolve` — re-proved, and it caught my own drift again.** `make publish-credential-proof` is green: 22 checks, 8/8 preimages, 8/8 patch sets applying, 21 fake `passage` lookups and 13 fake publisher invocations measured, `application_state: unapplied`, root `.env` at 6 assigned / 0 commented. Adding the Darwin coverage target to `src/agent-doc/Makefile` moved the twine recipe from line 334 to 340 and changed that file's preimage — the harness failed on it before I noticed, which is the third time it has earned its keep. Hash and hunk offset refreshed as `12e766d414`. Remaining blockers are unchanged and operator-only.

**`#lzgithubbudget20` — I could not verify this one, and did not pretend to.** `/users/btakita/settings/billing/*` returns 404 with a `user`-scope hint for this token, and the REST billing API exposes no payment-method field even with that scope. So the block is *unverified*, not re-confirmed, and there is no agent-executable slice. I left the item otherwise unchanged and recorded the limitation in its text.

**Verification.** Root `make check` exit 0, **9,820/9,820 tests**, `audit-docs` clean. Two earlier runs went red and I did not waive either:

1. `test_release_cadence_applies_only_to_macos_assets` failed because extracting the shared `gh` fetch dropped the phrase naming what the cadence gate refuses. That was a genuine regression in my change, not a brittle test — the fix threads the consequence in from each caller, so the cadence path still says it is "refusing to publish Darwin assets" and the coverage path states its own.
2. `queue_authority_observation_requires_exact_strike_application_receipt` failed once under the full 9,820-test run (retry receipt `None` instead of `Some(<projection_id>)`, 1.14s) and passes 5/5 in isolation at 0.02s. It waits on a bounded receive, so parallel load starves it. The third full run passed all 9,820, CI is green at HEAD, and nothing I changed compiles into `agent-doc-controller-io`. Filed as actionable rather than fixed, because a concurrent session holds uncommitted rustfmt edits across that exact file and editing it now would collide.

Also worth knowing: the background-task summary for both red runs reported "exit code 0" while `make check` had actually exited 2. The explicit `$?` capture is what caught it — a summary line is not a test result.

Three items queued, none of them gates. This session's tree also holds another session's uncommitted work, so the green suite is proof about my change, not about HEAD.
<!-- agent:boundary:ce9fbb66:lazily -->
<!-- /agent:exchange -->
 
## Queue

<!-- agent:queue preset="#spec-test-commit-push use subagents for fan-out tasks" priority go -->
- 🚧 #advance-review
- do [#lzauditrefresh]
- do [#lzpypiperishable]
- do [#lzstrikereceiptflake]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#lzauditrefresh] Regenerate the GitHub Actions artifact purge audit before the 7-day handoff guard expires at 2026-10-02T17:02:35Z, so [#lzagentdocartifactpurge] stays authorizable without --allow-stale-handoff. The generator is already parameterized (src/agent-doc/scripts/audit-actions-artifacts.py --repo --output-dir); a fresh run drops the 105 rows whose PyPI counterpart was pruned and re-dates the handoff. This replaces the basis of a pending operator authorization (2,781 candidates / 466 withheld, commit 4af68aa8f), so restate the new counts in that review item when it lands.
- [ ] [#lzpypiperishable] Classify PyPI-only durable counterparts as perishable in src/agent-doc/scripts/audit-actions-artifacts.py. Proven 2026-09-27: 27 agent-doc versions (0.35.0-0.35.37) lost their PyPI wheels within a day, killing 105 delete candidates, and 530 of the 2,666 remaining candidates rest on PyPI alone with the prune floor at 0.35.113 sitting six versions below the lowest surviving backer (0.35.119). Emit counterpart durability as its own column and summary so an operator can authorize release-asset-backed rows without inheriting PyPI decay risk.
- [ ] [#lzstrikereceiptflake] Make agent-doc-controller-io project_controller::tests::queue_authority_observation_requires_exact_strike_application_receipt deterministic. Under the full 9,820-test nextest run it failed with the retry receipt None instead of Some(projection_id) after 1.14s, while passing 5/5 in isolation at 0.02s; it waits on a bounded receive, so parallel load starves it. Not fixed 2026-09-27 because a concurrent session held uncommitted rustfmt edits across agent-doc-controller-io/src/project_controller.rs and editing it would have collided. Replace the timing budget with a deterministic barrier or SimWorld step.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#lzmacosrunner] [operator-decision, agent-doc/lazily-py] Current agent-doc policy intentionally keeps macOS out of GitHub Actions and uses operator-built Darwin assets via `make release-macos-assets TAG=<chosen-current-release>` no more frequently than weekly. Re-verified 2026-09-27: `make release-macos-cadence-check` still reports the weekly window open since 2026-09-22T16:41:49Z, and v0.35.398 (2026-09-15) is still the last release carrying both Darwin architectures. The policy's never-drop-either-deliverable half is now enforced instead of prose: `make release-macos-coverage-check` (commit ddb612833) names every release published after the last Darwin-complete one that ships an incomplete pair, and it currently fails naming five — v0.35.399, v0.35.400, v0.35.401, v0.35.406, v0.35.417. It is deliberately NOT a `make check` dependency, because clearing it needs Mac hardware. Keep lazily-py hosted macOS wheels unchanged and never drop either deliverable. Blocked until the operator runs the Mac-only release command or supplies the Mac SSH endpoint, username, architecture, and dedicated build-account confirmation for agent automation.
- [/] [#lzgithubbudget20] Set the btakita personal Actions product budget to $20/month with Stop usage enabled; retain $0 hard-stop budgets for Packages, Codespaces, Git LFS, and AI credits. Blocked until a valid payment method is added to the personal GitHub account. Checked 2026-09-27 and could NOT verify the current state from here: /users/btakita/settings/billing/* returns 404 with a user-scope hint for this token, and the REST billing API exposes no payment-method field even with that scope — so this block is unverified rather than re-confirmed. No agent-executable slice.
- [/] [#lzagentdocartifactpurge] [operator-decision, btakita/agent-doc] Read-only audit commit 4af68aa8f verified 2,781 exact durable-counterpart delete candidates totaling 45.56 GB and withheld 466 artifacts; the handoff is under src/agent-doc/tasks/agent-doc/artifact-purge-audit-2026-09-25/. The re-intersection and withheld-row refusal are now code, not prose: run `python3 scripts/purge-actions-artifacts.py` (commit 187f31622) from src/agent-doc. Its live dry run on 2026-09-26 planned 2,771 deletable artifacts (45.47 GB), skipped 10 already absent from the API, and refused 0. The destructive run is `--execute --authorize-deletion btakita/agent-doc --max-deletions 3000`; the 250 default cap will otherwise abort. Blocked solely on explicit destructive authorization; no artifact has been deleted.
- [/] [#lzambienttokenresolve] [operator-decision, agent-loop superproject] The ready-to-apply point-of-use credential plan is complete at tasks/software/plan-ambient-publish-token-point-of-use.md. The agent-executable prerequisite `#lzfakepublisherproof` is DONE: `make publish-credential-proof` (commits `cf2cbfda9`, `3e48cef93`) statically verifies all eight preimages plus `git apply --check`, then exercises all eight production call sites in success, failure, and empty modes against fake passage/cargo/maturin/bun/twine recorders — 22 checks, currently green, resolving no credential and invoking no publisher. It already caught real drift and refreshed two preimage hashes (`src/agent-doc/Makefile`, root `Makefile`) plus one hunk offset. Remaining blockers are operator-only: a quiet window for application, rotation of the Cargo, npm, and PyPI credentials, and one manual step the harness cannot do — rerunning the plan's step-5 name loop in a real INTERACTIVE shell after application, because a non-interactive shell does not fire mise's activation hook and forcing it would resolve real credentials. Do not resolve or print credential values during preparation.
<!-- /agent:review -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/lazily.done.md -->

<!-- completed work archived in tasks/software/lazily.done.md -->
<!-- /agent:done -->
