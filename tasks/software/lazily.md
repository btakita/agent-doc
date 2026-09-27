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

### Re: regenerate the artifact purge audit — opus-5 · 2026-09-27T01:48-04:00

> **Queue prompt:**
>
> do [#lzauditrefresh]


Audit regenerated and the authorization basis re-dated. Two commits on `btakita/agent-doc` main, both pushed: `667eb4766` (the refresh) and `878396cfd` (a tripwire follow-up).

**The refresh.** `python3 scripts/audit-actions-artifacts.py --repo btakita/agent-doc --output-dir tasks/agent-doc/artifact-purge-audit-2026-09-27` (36s, read-only). Against the 2026-09-25 handoff:

| | 2026-09-25 | 2026-09-27 |
|---|---:|---:|
| Live nonexpired artifacts | 3,247 (52.90 GB) | 3,235 (52.80 GB) |
| Verified delete candidates | 2,781 (45.56 GB) | **2,666 (43.83 GB)** |
| Withheld | 466 (7.35 GB) | **569 (8.97 GB)** |
| — PyPI platform file missing | 395 | 500 |
| PyPI releases visible to the audit | 185 | 151 |

The +105 in the PyPI-missing class is exactly the decay `[#lzagentdocartifactpurge]` recorded: PyPI pruned wheels for 34 versions, and the fresh audit now withholds those rows at generation time instead of leaving the executor to refuse them live.

**Proof the two agree again.** The live dry run (`--max-deletions 3000`, no `--execute`) plans **2,666 deletions, 0 refused, 0 skipped** — the first clean intersection since 2026-09-25. Candidate composition: 2,136 backed by GitHub Release assets (34.83 GB), 530 by PyPI files alone (9.01 GB), lowest surviving PyPI-backed version `v0.35.119`. That 20% PyPI-only residual is the risk the authorization still carries; it did not change shape, only date.

**New deadline: 2026-10-04T05:40:26Z.** Generated 2026-09-27T05:40:26Z, so `[#lzagentdocartifactpurge]` is authorizable for seven days without `--allow-stale-handoff`. I updated that review item with the new counts, directory, and deadline.

I pointed `DEFAULT_HANDOFF_DIR` at the new directory and deleted the superseded 2026-09-25 handoff — git history keeps it, and leaving two handoffs in the tree invites `--handoff-dir` at expired data. Git recorded the four files as renames, so the diff is 194 insertions / 206 deletions rather than a 1.4 MB duplication.

**The tripwire follow-up.** The self-test pins the committed handoff counts so a swapped handoff cannot be adopted unreviewed. On this legitimate regeneration it failed with a bare `AssertionError: 2666` — no pin, no remedy. It now names observed vs pinned counts, the handoff directory, and both readings (regenerated → restate the counts in the authorization and update the pins; otherwise → the handoff was replaced without review). Verified by mutating the pin back to `(2781, 466)` in place: the message fires as written, and `make artifact-purge-check` is green on the restored file.

**Verification.** `make artifact-purge-check` exit 0 (the `make check` leg that owns this file) plus the live dry run above. I did **not** run full `make check`: the change touches no Rust, and the submodule tree currently holds another session in-flight edits across 13 Rust files (`src/main.rs` modified eight minutes before this commit), so a suite result there would attest to their code, not mine — and committing or building it was the hazard to avoid. `git commit --only` kept the commits to my five paths; their edits remain uncommitted. Latest CI run `36297051970` was `in_progress` when checked (started 05:23Z, from an earlier push) — recorded, not awaited.

No `make install`: `scripts/purge-actions-artifacts.py` is not in the binary, so the advisory `stale_install` warning is unrelated to this change and I left the install to whichever cycle owns the Rust work in flight.

### Re: classify PyPI-only counterparts as perishable — opus-5 · 2026-09-27T02:02-04:00

> **Queue prompt:**
>
> do [#lzpypiperishable]


Done, and it went further than a column: the operator can now authorize the non-decaying half alone. Pushed as `434b32548`.

**The distinction the audit was missing.** Both counterpart classes are *exact* at generation time, so candidacy never depended on which one backed a row — and that was the gap. A GitHub Release asset survives until someone deletes the release. A PyPI file does not. Durability is how long the candidacy proof lasts, which is a different question from whether it holds today, and the handoff answered only the second.

**Generator** (`audit-actions-artifacts.py`):

- `counterpart_durability(kind)` is the one definition — `github_release` → `durable`, `pypi` → `perishable`. An undeclared kind **raises** instead of defaulting, so a future counterpart class must state its durability before it can be offered as a candidate.
- `counterpart_durability` is now a column on both CSVs, and `candidate_durability` a summary block with count, bytes, counterpart kinds, and the **version span** per class. The span is the load-bearing part: it names the oldest version whose counterpart still exists, which is exactly what you compare against the prune floor. The handoff now says `v0.35.119` itself instead of leaving it to be rederived by hand.
- The report gained a `## Counterpart durability` section naming both classes and the decay evidence.

**Executor** (`purge-actions-artifacts.py`):

- `--durability durable|perishable|all` (default `all`). An excluded row is **refused** as `durability_excluded`, not silently dropped, and `--only` cannot widen the plan past the filter.
- `load_handoff` now takes the generator module, so durability has one definition across both scripts. It refuses a handoff whose column disagrees with the generator's classification, whose `candidate_durability` summary disagrees with the CSV, or that has **no** durability column — that last one names the regenerate remedy rather than defaulting a perishable row into `durable`, which is the failure that would have silently re-armed the original bug.
- I deliberately did *not* add `counterpart_durability` to the live counterpart-drift tuple: durability is derived from the kind, and the load-time check already proved the column agrees, so kind drift is the only reachable path. Adding it would have been untestable code.

**The two authorizable options now**, both proven by live dry run:

| | Artifacts | Bytes | PyPI decay risk |
|---|---:|---:|---|
| default (all) | 2,666 | 43.83 GB | 530 rows rest on PyPI alone |
| `--durability durable` | 2,136 | 34.83 GB | none |

The filter refuses exactly 530 rows — the same 530 the summary classifies as perishable (9.01 GB, oldest still-backed `v0.35.119`, six versions above PyPI's `0.35.113` prune floor). Durable rows span back to `v0.34.69`. I updated `[#lzagentdocartifactpurge]` with both options.

**Verification.** Seven new self-test regressions: both filter directions, the `--only` widening refusal, the report fields, and the three load guards. Then I attacked each new guard rather than reasoning about it — neutering the filter, the per-row classification check, the summary check, and the missing-column check each reddened the suite in place, and all four restored green. Live: unfiltered plans 2,666 with 0 refused; `--durability durable` plans 2,136 refusing 530. Regenerating the handoff moved withheld 569 → 571 (two artifacts from the CI run that started at 05:23Z), so the self-test's committed-handoff pin is now `(2666, 571)`.

Same scope note as the last cycle: `make artifact-purge-check` is the `make check` leg that owns these files and it is green, but I did not run the full suite — the change is Python-only and the submodule tree still holds another session's uncommitted edits across 14 Rust files. `git commit --only` kept this to my six paths; superproject pointer is `d0c8b7a75`. Exchange is at 255 lines / 10 response sections, noted and still draining.

### Re: do [#lzstrikereceiptflake] — opus-5 · 2026-09-27T02:43-04:00

> **Queue prompt:**
>
> do [#lzstrikereceiptflake]


Fixed and pushed as `agent-doc@545b0792c`.

**Root cause.** The strike worker writes the document *first* and publishes its application receipt *second* (`complete_answered_free_text_strike` sets `answered_free_text_strike_applied` only after the invocation returns `Ok`). The test polled the document text for `queue: stop`, then read the receipt with no wait at all — so the successful file write was the signal, and the receipt `set` could still be in flight. Under the full parallel run the read landed first and observed `None`. The 3 s budget was never the problem; the missing barrier was.

**Fix.** Added an Effect-driven `await_source_map_entry` barrier over a `ThreadSafeSourceMap` entry: it subscribes to the entry (membership edge plus value edge, the same pattern the production strike Effect uses), publishes into a `parking_lot` `Mutex`/`Condvar` pair when the value satisfies a predicate, and releases on the exact `set`. Both eventual receipts in the test now use it — the adapter failure and the application receipt — and the two polling loops are gone. Because the receipt strictly follows the write it acknowledges, the receipt barrier now also orders the document-text read that follows it, so the ordering is asserted rather than assumed. The remaining 60 s timeout is a liveness backstop that only reports a lost publication; it no longer carries the correctness of the wait.

The failure-side loop got the same treatment: barrier on `answered_free_text_strike_failure` becoming `Some`, then one `document_queue_authority_observe` call that must return `Err` — which is a stronger contract than "poll until it eventually errors".

**Verification.** `make test` green, exit 0: **9845 tests run, 9845 passed**, 259 skipped, 0 `result: FAILED`. The target test passed inside that full parallel run, and 1/1 in isolation at 0.01 s. `cargo clippy -p agent-doc-controller-io --all-targets -- -D warnings` exit 0; `cargo fmt --check` clean for the file I touched.

Two notes on the shared tree. A concurrent session held uncommitted rustfmt hunks in the same file (at lines ~3653 and ~10251, far from the test) plus edits in five other files, so I staged a hand-built blob — HEAD plus my hunks only — and verified it differs from HEAD solely inside lines 17848–18027 and from the worktree solely by their two reformat hunks. Their edits survive uncommitted. The first `make test` did die, but in the doctest phase on a missing `libagent_doc_document_realtime-*.rlib` that a concurrent `cargo check` removed mid-run; doctests pass on their own (exit 0) and the clean re-run of `make test` was green end to end.

CI run 36299889075 was still `in_progress` at closeout, so this rests on local evidence. No `make install` — the change is test-only, and a mid-session install against the live supervisor risks stranding this document's editor replica.

### Re: #advance-review — opus-5 · 2026-09-27T03:17-04:00

> **Queue prompt:**
>
> #advance-review

I re-attacked all four review gates instead of restating them. Three did not move. One was **red**, and the fix is committed.

**`#lzambienttokenresolve` — the gate was broken, not just blocked.** `make publish-credential-proof` exited 2: `src/agent-doc/Makefile` recorded preimage `3de14f9d851a…` but measured `a0931f885d4f…`. Cause is `e7c0694de` (`build(pypi): measure the storage ceiling instead of asking a human to unlock it`), which moved the twine recipe from line 340 to line 355. The review text claimed "Re-verified 2026-09-27: 22 checks green" — that claim was stale by one commit, and a green-sounding gate over a non-applying patch is the worst state this plan can be in, because the operator's application step is exactly where it would surface.

Refreshed both halves — the recorded hash and the zero-context hunk header (`@@ -340 +340,4 @@` → `@@ -355 +355,4 @@`) — committed as `8c7d9face4`. Re-run is green: 22 checks, 0 failures, 8/8 preimages match, 8/8 patch sets apply, 21 fake `passage` lookups and 13 fake publisher invocations measured, `application_state: unapplied`, root `.env` at 6 live / 0 commented. No credential was resolved and no publisher was invoked. This is the **fourth** drift; the recurrence is the reason for the new backlog item below.

**`#lzagentdocartifactpurge` — one number in the gate was misleading.** The handoff is intact (only `artifact-purge-audit-2026-09-27` on disk) and the 7-day guard leaves 166.7 h, aborting after `2026-10-04T05:56:08Z`. I re-measured the PyPI side directly: the prune floor is still `0.35.113` and all 151 release entries still carry files, so nothing decayed since the audit. But "six versions above the 0.35.113 prune floor" counts version numbers, not releases — in live releases, `v0.35.119` is the **third** entry from the floor (`0.35.113`, `0.35.114`, `0.35.119`). Option (a)'s 530 PyPI-only rows sit two live releases from the edge, not six. That does not change which options are authorizable; it does make option (b) the conservative default more clearly than the prose implied.

**`#lzmacosrunner` — no movement, and that is itself the finding.** `make release-macos-cadence-check` exit 0, weekly window open since `2026-09-22T16:41:49Z`; `make release-macos-coverage-check` exit 2 naming the identical five releases (v0.35.399, v0.35.400, v0.35.401, v0.35.406, v0.35.417). An unchanged list means no release was cut since the last sweep, so the gap is not widening. Still needs Mac hardware.

**`#lzgithubbudget20` — still unverifiable, with one new measurement.** Both `/users/btakita/settings/billing/actions` and `/users/btakita/settings/billing/usage` reproduce 404 plus the `user`-scope hint, and that scope exposes no payment-method field anyway. What I could measure: the five most recent agent-doc CI runs all started, and both completed ones are green. So Actions execution is not blocked — this is spend-limit hardening, not an outage. That reframes the urgency downward; the operator block is unchanged.

**New actionable item `#lzpatchcontextpin`.** The offset half of the credential-plan drift has now fired four times in a few days, each time needing a hand-edited absolute line number — which is a mis-pin risk in the one artifact that must apply cleanly under an operator. Context-bearing diffs let `git apply` locate hunks by content and make that half self-healing, while the SHA-256 preimage pin stays exactly as-is: it is the review tripwire and must still go red on any edit to a patched file.

**Verification and scope.** `make publish-credential-proof` is the `make check` leg that owns the changed file and it is green at exit 0. I did **not** run full `make check`: the change is markdown-only, touches no Rust, and the `src/agent-doc` submodule currently holds another session's six uncommitted Rust files (`agent-doc-commit-io/src/lib.rs`, `agent-doc-commit-io/tests/git.rs`, `agent-doc-controller-io/src/project_controller.rs`, `.../project_controller/rpc.rs`, `agent-doc-queue/src/document_queue.rs`, `src/plugin_activation.rs`) plus two in `src/corky` — running its suite would test that session's tree, not mine. Three CI runs were in progress at closeout; the two most recent completed ones are green. No `make install`: the stale-install advisory names two missing commits, but this document does not own agent-doc development, and a mid-session install would strand this document's editor replica.

Exchange is at 312 lines across 12 response sections, over the warn thresholds; the queue is active so I am not pausing to compact.
<!-- agent:boundary:ce9fbb66:lazily -->
<!-- /agent:exchange -->
 
## Queue

<!-- agent:queue preset="#spec-test-commit-push use subagents for fan-out tasks" priority go -->
- 🚧 #advance-review
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#lzpatchcontextpin] Convert the eight `--unidiff-zero` patch blocks in tasks/software/plan-ambient-publish-token-point-of-use.md to context-bearing diffs so `git apply` locates each hunk by content instead of by absolute line number, and drop `--unidiff-zero` from the apply check in scripts/proof-point-of-use-publish-credentials.py. Keep the SHA-256 preimage pin exactly as-is — it is the review tripwire and MUST still go red on any edit to a patched file; do not add an auto-refresh mode, which would bless a hostile edit. Motivation: the offset half of that drift has fired four times in days (latest e7c0694de moved the agent-doc twine recipe 340->355), each firing needing a hand-edited absolute line number, itself a mis-pin risk in the one artifact that must apply cleanly under an operator. Verify with `make publish-credential-proof` green plus a deliberate line-shift perturbation of one patched file that leaves the hunk applying while the hash still goes red — attack the guard, do not reason about it. Unblocks part of [#lzambienttokenresolve].
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#lzmacosrunner] [operator-decision, agent-doc/lazily-py] Current agent-doc policy intentionally keeps macOS out of GitHub Actions and uses operator-built Darwin assets via `make release-macos-assets TAG=<chosen-current-release>` no more frequently than weekly. Re-checked 2026-09-27T07:12Z with NO movement: `make release-macos-cadence-check` exit 0, weekly window still open since 2026-09-22T16:41:49Z, and v0.35.398 (2026-09-15T16:41:52Z) is still the last release carrying both Darwin architectures. The never-drop-either-deliverable half is enforced instead of prose: `make release-macos-coverage-check` (commit ddb612833) exits 2 naming the IDENTICAL five incomplete-pair releases — v0.35.399, v0.35.400, v0.35.401, v0.35.406, v0.35.417 — so no release was cut since the last sweep and the gap is not widening. It is deliberately NOT a `make check` dependency, because clearing it needs Mac hardware. Keep lazily-py hosted macOS wheels unchanged and never drop either deliverable. Blocked until the operator runs the Mac-only release command or supplies the Mac SSH endpoint, username, architecture, and dedicated build-account confirmation for agent automation.
- [/] [#lzgithubbudget20] Set the btakita personal Actions product budget to $20/month with Stop usage enabled; retain $0 hard-stop budgets for Packages, Codespaces, Git LFS, and AI credits. Still NOT verifiable from here, reproduced 2026-09-27T07:13Z: both /users/btakita/settings/billing/actions and /users/btakita/settings/billing/usage return 404 with a `user`-scope hint for this token, and the REST billing API exposes no payment-method field even with that scope — so this block stays unverified rather than re-confirmed. Newly measured this cycle: the five most recent agent-doc CI runs all STARTED and both completed ones are green, so Actions execution is not currently blocked — this is spend-limit hardening, not an outage. Blocked until a valid payment method is added to the personal GitHub account. No agent-executable slice.
- [/] [#lzagentdocartifactpurge] [operator-decision, btakita/agent-doc] Read-only audit regenerated 2026-09-27T05:56Z WITH counterpart durability classification (commits 667eb4766, 434b32548); src/agent-doc/tasks/agent-doc/artifact-purge-audit-2026-09-27/ is confirmed the ONLY handoff on disk, so only one basis can be authorized. The re-intersection, withheld-row refusal, and durability filter are code, not prose: run `python3 scripts/purge-actions-artifacts.py` from src/agent-doc; the destructive run is `--execute --authorize-deletion btakita/agent-doc --max-deletions 3000` and the 250 default cap otherwise aborts. TWO authorizable options, both proven by live dry run at 2026-09-27T05:58Z: **(a) full plan** — 2,666 artifacts (43.83 GB), 0 refused, 0 skipped; **(b) `--durability durable`** — 2,136 artifacts (34.83 GB), refusing exactly the 530 rows whose only counterpart is a PyPI file. Option (b) carries no PyPI decay risk. Option (a) additionally accepts that 530 rows (20%, 9.01 GB) rest on PyPI alone. Re-measured 2026-09-27T07:14Z: the prune floor is still 0.35.113 and all 151 release entries still carry files, so nothing decayed since the audit — but the margin is thinner than version numbering suggests. v0.35.119, the oldest PyPI-only-backed version, is only the THIRD live release above the floor (0.35.113, 0.35.114, 0.35.119), not six versions clear, so prefer (b) unless the extra 9.01 GB is needed. 2,136 rows rest on GitHub release assets, which have not decayed. 571 artifacts are withheld at audit time and are never deletable. Hard deadline: the 7-day `--max-handoff-age-days` guard aborts the whole run after 2026-10-04T05:56:08Z (166.7 h remaining at re-check) unless the audit is regenerated or `--allow-stale-handoff` is passed. Blocked solely on explicit destructive authorization; no artifact has been deleted.
- [/] [#lzambienttokenresolve] [operator-decision, agent-loop superproject] The ready-to-apply point-of-use credential plan is complete at tasks/software/plan-ambient-publish-token-point-of-use.md, and its agent-executable prerequisite [#lzfakepublisherproof] is DONE: `make publish-credential-proof` (commits `cf2cbfda9`, `3e48cef93`) statically verifies all eight preimages plus `git apply --check`, then exercises all eight production call sites in success, failure, and empty modes against fake passage/cargo/maturin/bun/twine recorders, resolving no credential and invoking no publisher. It caught its own drift a FOURTH time on 2026-09-27, and this time the gate was RED before the fix: `src/agent-doc/Makefile` recorded 3de14f9d851a… but measured a0931f885d4f…, drifted by e7c0694de (`build(pypi): measure the storage ceiling instead of asking a human to unlock it`), which moved the twine recipe from line 340 to 355. Refreshed hash and zero-context hunk offset committed as `8c7d9face4`; the re-run is green — 22 checks, 0 failures, 8/8 preimages match, 8/8 patch sets apply, 21 fake passage lookups and 13 fake publisher invocations measured, application_state `unapplied`, root `.env` at 6 live / 0 commented. See [#lzpatchcontextpin] for removing the recurring offset half. Remaining blockers are operator-only: a quiet window for application, rotation of the Cargo, npm, and PyPI credentials, and rerunning the plan's step-5 name loop in a real INTERACTIVE shell after application, because a non-interactive shell does not fire mise's activation hook and forcing it would resolve real credentials. Do not resolve or print credential values during preparation.
<!-- /agent:review -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/lazily.done.md -->

<!-- completed work archived in tasks/software/lazily.done.md -->
<!-- /agent:done -->
