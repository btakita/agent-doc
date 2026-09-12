---
agent_doc_session: d818612d-3614-4c43-b5ec-83923406aa46
resume:
  claude: d6379dbb-42f5-4658-8da6-e0251c5eaa18
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/44ae7f3a5c03b112ffe17aa5c85d000e3d79a6e9a4875e16442309ebf1c06a1c-20260912-041014.md`*

Compacted content:
- Archived 2 response topic(s): do[#lzktblockwalk]; do[#lzktbindpending]
- Prior summary/context: prior compacted content: Archived 24 response topic(s): #lzpystructcell; what shipped; #lzpypromegress; 21 more

### Re: do[#lzktreactivemodelfailures] — opus-5

> **Queue prompt:**
>
> do [#lzktreactivemodelfailures]


Done — `lazily-kt 8aa3ba7` (CI ✅), superproject `8d2cf9965`. The reactive-graph blocker is settled, so `#lzktbindpending`'s 88 reactive-graph sites are unblocked. **The migration itself did not happen and the pin did not move** — still 512, ledger unchanged, verified.

The decision: a **value** divergence is a property of one execution model, so all three should report and accumulation is right. A **coverage** defect — "this runner never read `observed_count`" — is a property of the *runner*, identical in all three passes by construction, so accumulating it three times reports the same thing three times and failing fast is right. They were never in conflict; a runner can have both.

**The caveat became the real fix, which is the outcome I hoped for but didn't assume.** I offered a fallback of documenting the weakness if strengthening the tracker turned into an API redesign. It didn't. `assertKeyWith` books a key asserted the moment a comparison *runs*, whatever it concluded — which in an accumulating runner quietly degrades rung 3 to "a comparison happened" and makes the tracker's signal a restatement of the accumulator's. The new `assertKeyOutcome(key) { want -> Boolean }` takes the **outcome**, so a swallowed divergence leaves the key consumed-but-unasserted and rung 3 fires on its own whether or not anyone reads the accumulator's report. That is strictly stronger for any caller with this shape, not just this one, and it costs the caller one Boolean.

Two details worth noting because they are the difference between a fix and a fix that survives. `AccumulatingRunnerCoverageTest` pins both halves against a **stand-in accumulator** rather than against the runner, so the tests survive a restructure of the thing they are about — and it deliberately pins `assertKeyWith`'s *weaker* behaviour as well, so the reason `assertKeyOutcome` exists cannot be lost to a future reader who sees two similar methods. Both probes were mutation-checked rather than trusted green: making `assertKeyOutcome` ignore its outcome reddens the swallowed-divergence test, and restoring the missing assertion reddens the unread-key test.

The decision is written at the `expect` loop where someone hitting a failure will actually be looking, including the plain statement that the wiring is not done.

**Queue-order note, third instance.** This item was 9th in the reorder I committed last cycle and arrived as the head anyway. Same mechanism as before: the closeout that filed it hit the retained-write deferral, so the placement path never ran and the next preflight's backlog→queue sync reinstated it in backlog order. It is now fighting explicit `--backlog-reorder` decisions, not just the `append` default, which makes `#lzfollowupqueueorder` a bit more than a nicety — I have left it queued rather than jumping it, since the work at the head has been genuinely next-most-relevant each time.
<!-- agent:boundary:ce9fbb66:lazily -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-commit-push use subagents for fan-out tasks" priority go -->
- ~~do [#lzktreactivemodelfailures]~~
- do [#lzrsblockwalk]
- do [#lzdartboundfloor]
- do [#lzdartarrayblocks]
- do [#lzorderinganchors]
- do [#lzktareapartition]
- do [#lzscenariofloormissing]
- do [#lzcireachsync]
- do [#lzfollowupqueueorder]
- do [#lzktbindpending]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#lzrsblockwalk] [lazily-rs] rs`s rung-0 walk is the same narrow shape lazily-zig retired in #lzzigblockwalk: the top-level `assertions` object plus the `assertions` object of each element of the top-level frames/scenarios/rejects arrays, and nothing else — 36 sites / 30 distinct digests of the 771/661 the same 150 opened fixtures carry under every block name at every depth, so 4.7% of sites and 4.5% of digests. The entire remainder is the four names this walk never looks at: `expected` 434 sites/359 digests, `expect` 295/266, `expect_initial` 3/3, `expect_after` 3/3 — and ZERO `assertions` blocks are missed at any depth, so the gap is purely the name set. Widen record_declared_blocks() in tests/common/mod.rs to the whole block-name set at every depth (objects only, emit-and-do-not-descend, name-preferring list labels), mirror it in the guard`s derivation so the one-walk-two-callers property holds, then bind or excuse every block it surfaces — expect runner work, not a guard edit. In lazily-zig the same widening surfaced 204 unbound digests over 239 sites across nine runner families and took a full cycle. Verify against `make corpus-blocks-report` in lazily-spec. Repo: src/lazily-rs CLONE-DIVERGENCE OBLIGATION, moved here from #lzrunnerownjsonclone because a narrow walk is the only place it can hide: when this walk widens, expect a runner/loader digest divergence among the newly surfaced unbound blocks and look for it DELIBERATELY before treating them as separate gaps. lazily-cpp widened and 71 of its 96 surfaced sites turned out to be ONE bug — its reactive-graph runner re-parsed the block and its number parse dropped the raw token, so the clone digested `5` as `#5.000000` where the loader digested `#5`, and the clone`s own comment falsely asserted content-identity. See docs/conformance.md `#lzrunnerownjsonclone` in lazily-spec 347f6fd for the rule: prefer routing the tracker at the loader`s own value over asserting equality between two parses, and if a second parse is unavoidable assert in-run that the clone`s digest equals the loader`s for a number-bearing, a string-bearing and a nested block, then perturb a number`s spelling (5 vs 5.0 vs 5e0) in a scratch corpus to confirm both sides still agree.
- [ ] [#lzdartboundfloor] [lazily-dart] scripts/check-unbound-blocks.py still carries two typed `>=` floors that #lzblockmagnitudeaudit deliberately left alone because they are different rungs: MIN_BOUND_BLOCKS=697 (blocks BOUND) and MIN_BLOCK_FIXTURES=144 (fixtures opened). MIN_BOUND_BLOCKS is now the last number in this repo a person re-pins by hand — it read 697 when pinned and the run reports 700 today, so it is already 3 slack and guarding nothing in that range, the exact drift the block magnitude just stopped. MIN_BLOCK_FIXTURES is now REDUNDANT: the site-set equality subsumes it, since the derived site set is keyed by fixture and a missing fixture takes its sites with it. Derive the bound count the same way (declared minus the excuse ledger, asserted EQUAL) or retire it in favour of the unbound-set check that already exists, and delete MIN_BLOCK_FIXTURES. Repo: src/lazily-dart
- [ ] [#lzdartarrayblocks] [lazily-dart] dart`s walk is object-valued only, so array-valued tracked keys contribute no site: the same 144 opened fixtures give 725 sites / 616 digests under dart`s rule and 737/628 when array ELEMENTS count. The gap is exactly 12 blocks and they are all in one fixture — `signaling/anti_spoof_session.json`, whose eight array-valued steps[].expect keys carry 1+2+3+1+1+1+1+2 elements. lazily-js instruments each element (which is why js derives 747 where dart derives 725); dart`s objects-only rule sees none of them, so whatever those 12 blocks assert — one of them is the anti-spoof invariant the fixture exists for — is outside rung 0 in this binding. Decide deliberately: either instrument array elements as js does, or record why an array-valued block has no bindable site here. Related and worth settling in the same pass: dart reads its block-name set out of the tracker, so a name the CORPUS grows is invisible on both the inventory and the derivation at once (a NAME-OPEN `^(assert|expect)` scan finds nothing beyond the five names today — `expected` 405, `expect` 294, `assertions` 32, `expect_initial` 3, `expect_after` 3 — but that is today`s corpus, not a guarantee). Repo: src/lazily-dart
- [ ] [#lzorderinganchors] [lazily-spec] scripts/check-assertion-ordering.py ORDERED_CHECKS anchors its ordering contract on the EXACT floor literals the bindings are now removing - regexes like 'assert accepted == 6', 'assert replayed == 12', 'assert.equal(accepted, 10)' for py/js/cs. lazily-py could not delete three of its floors this cycle and shipped them as DEAD ANCHORS, kept only to satisfy this guard, with the live constant-free assertion sitting above them. That is debt the guard created. Re-anchor on something stable that is not a value - a named helper call, or a marker the runner declares - so removing a constant does not break an unrelated ordering contract. Note the anchor must also be UNIQUE in the file (py tripped 'anchors are not unique' by quoting the literal in an explanatory comment). Repo: src/lazily-spec, then src/lazily-py to drop the dead anchors.
- [ ] [#lzktareapartition] [lazily-kt] check-conformance-coverage.sh declares REQUIRED_AREAS (27 areas) with no EXCUSED_AREAS complement, so the area list scopes the AUDIT and not the set the suite OPENS: kt opens 148 fixtures / replays 157 scenarios where deriving from the areas alone gives 147 / 151. lazily-spec's check-corpus-floors.mjs therefore declines to derive any floor for kt (it would otherwise have called an honest MIN_FIXTURES=148 unsatisfiable). lazily-cpp pairs the two arrays and its own guard enforces the exact complement, and there the derivation is exact. Add the EXCUSED_AREAS complement (or the equivalent partition assertion) so kt's floors become auditable from the corpus side. Repo: src/lazily-kt
- [ ] [#lzscenariofloormissing] [lazily-py, lazily-kt, lazily-gd] No MIN_SCENARIOS floor is declared at all - lazily-spec's check-corpus-floors.mjs reports this rather than failing, because lazily-kt deliberately retired MIN_FIXTURES for an area-partition assertion and that is a stronger invariant. But a binding with NEITHER is relying on nothing: py replays 151 scenarios and kt 157 with no positive-evidence floor under either, so a run that silently stopped replaying scenarios would report green. Derive each floor from the gate's own output, add it, and prove it exact by failing at n+1. lazily-gd is staged and may legitimately have nothing to floor yet - check before adding. Repos: src/lazily-py, src/lazily-kt, src/lazily-gd
- [ ] [#lzcireachsync] [lazily-rs, lazily-py, lazily-js, lazily-kt, lazily-go, lazily-cpp, lazily-cs, lazily-dart, lazily-zig] The ten copies of scripts/check-ci-reach.sh are no longer byte-identical. Before #lzgdcireach the nine all carried md5 da1444b38e93 / 570 lines — an invariant nothing enforces but which is plainly deliberate, since the script has no shared home (unlike check-assertion-ordering.py, which lives in lazily-spec and is invoked across the sibling boundary). lazily-gd 913edc8 added `make_invokes_ancestor`: a CI step running `make <ancestor>` reaches every target in that ancestor`s prerequisite closure, walked with the same `prereqs_of` the main closure uses. Without it a verbatim copy reported all six of gd`s gates unreachable, because gd`s CI is a single `make check` step. THE NO-OP PROOF IS ALREADY DONE, so this item is a port plus verification rather than a design question: the new script was run in each of the nine and produced output BYTE-IDENTICAL to the old one, exit 0 in all eighteen runs — because none of the nine invokes `make check` from a `run:` step (each runs the underlying commands per gate; the only real `run:` body in any of them containing the phrase "make check" is this guard`s own refusal text, which is the comment-vs-command trap its header warns about). So: copy gd`s version into the nine, re-run each binding`s `ci-reach` target and confirm output is unchanged, then `make check` per repo and CI. Worth deciding in the same pass whether nine-plus-one hand-synced copies should keep being hand-synced at all — the alternatives are a shared home in lazily-spec (which would make every binding`s own guard depend on the sibling checkout, a cost the current design deliberately avoids) or a guard that asserts the copies are identical. Note for whoever takes it: the file is 641 lines in gd now, and the ONLY behavioural difference is the two new functions plus one call site and the header sentence, so a diff against gd is the cheapest review. Repos: the nine above
- [ ] [#lzfollowupqueueorder] [process] Follow-up items filed by a drain cycle keep preempting the queue they were filed from, because `--backlog-add` prepends by default and the queue mirror honours that. Three consecutive cycles filed an item that became the very next head — #lzblocksitepin, then #lzexpectedkeyorder, then #lzgdorderingguard, then #lzgdcireach — each pushing #lzcppblockwalk one place further back, so the drain went depth-first into gd tooling while the three walk-widening items (cpp/kt/rs) and the three older items (#lzorderinganchors, #lzktareapartition, #lzscenariofloormissing) sat untouched. Prepending is the right DEFAULT — a follow-up from the turn that just ran is usually the most task-relevant work, which is exactly why the flag exists — but it is the wrong choice for a follow-up that is merely ADJACENT rather than more urgent, and nothing currently prompts the agent to make that judgement. Concretely: prefer `--backlog-queue-placement append` for a follow-up that is not more urgent than the existing head, and consider whether the agent-doc skill should say so at the point where it tells the agent to capture follow-up work (runbooks/respond.md, the backlog-capture rule) rather than leaving the default to decide. Worth checking whether the same asymmetry applies to `--backlog-add-after`/`--backlog-add-before`, which the queue mirror also honours. This item deliberately uses append on itself. Repo: src/agent-doc (skill/runbook text), or file against tasks/agent-doc/ if it is treated as a product change
- [ ] [#lzktbindpending] [lazily-kt] PHASE 2 of #lzktblockwalk, IN PROGRESS — 28 of 540 sites bound, pin re-pinned DOWNWARD 540 -> 512, 512 remaining across 13 areas. Migrate kt`s remaining bind-pending assertion-block sites onto AssertionKeys: the block is reached and the runner DOES assert its keys, but by direct indexing rather than through a tracker, so rung 0 cannot see the bind and every rung above it is scoped to blocks a tracker bound and therefore says nothing about these. They are enumerated per site in scripts/conformance-unbound-blocks.txt with reason class `bind-pending`, and the guard asserts that count as an EQUALITY failing in BOTH directions and derived from the RUN rather than the ledger, so a migration cannot land without moving the number in the same commit and cannot silently regress upward. FIRST PASS (lazily-kt 93420ff) bound 5 areas over 4 block shapes to prove the recipe generalises: ipc/root 1, familysync 3, protobuf 6, membership 9, message-passing 9. THE MEASURED GAP RATE IS THE REASON THIS ITEM IS BIGGER THAN ITS SITE COUNT: every one of those 28 blocks failed a higher rung once bound, and HALF needed a real fix rather than a routing change — 6 distinct defect classes, each a hole the narrow walk was hiding. `ordinary_snapshot_count` (6 blocks) compared against the LITERAL 0, so the key asserted the corpus against itself and a binding admitting a purposeless snapshot passed; `rpc.terminal_status` and `conflict` were PRESENT IN THE FIXTURE AND NEVER READ, the latter being the headline claim of terminal_conflict_fail_closed; `ignored_frame_indices` was asserted one way only; `target_epoch_bumped` was read but asserted only when TRUE, so a `false` upstream would have vanished the assertion; `*_after_frame_index` let the fixture`s own value steer which assertion to make instead of comparing against the run`s. Plus 7 object-valued keys consumed with no key-set check. If that rate holds the remaining 512 sites carry on the order of a hundred more real defects — but treat that as a measured rate from one sample, not a forecast: the easy classes may be exhausted first, so RE-MEASURE the per-area gap rate each pass and report it. REMAINING AREAS: collections 132, reactive-graph 88, stdlib 54, ingress 42, rateshape 33, coordination 29, windowing 23, egress 22, resilience 21, service 20, temporal 16, presence 16, lossless-tree 16. Take one or two per pass; delete exactly that area`s ledger entries and re-pin the equality down by exactly that count in the same commit; verify the guard fires on an off-by-one in BOTH directions. Every higher-rung failure a migration surfaces must be FIXED, never excused — a bound-but-failing block is a different state from an unbound one and must not be laundered back into the ledger. reactive-graph (88) is BLOCKED on #lzktreactivemodelfailures and should not be started until that is settled. Gradle proof discipline applies: JUnit XML timestamps, not task lines, and `--rerun-tasks` if a manifest may be stale. Repo: src/lazily-kt
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#lzagentloopcibilling] [operator-action] GitHub Actions is blocked on btakita/agent-loop by a spending limit, and has been since 2026-08-09. Every run of both workflows (Agent Doc, Lazily network suite) reports "The job was not started because recent account payments have failed or your spending limit needs to be increased" — no job starts, so there are no logs and nothing to debug in the workflow itself. Diagnosed precisely: btakita/agent-loop is PRIVATE, so it consumes paid Actions minutes; btakita/tsift, btakita/agent-doc and every lazily-hub/* binding are PUBLIC and run on free minutes, which is why binding CI has been green throughout while the superproject has been dark. Last successful run 31293624224 at 2026-08-09T04:00:25Z; first billing failure 31297858292 at 2026-08-09T05:58:50Z. COVERAGE LOST for that whole window: the `check` job (make check under tmux) and the `tmux-ci` job (make tmux-ci) in agent-doc.yml, plus the tiered `network-suite-*` targets in lazily-network-suite.yml — none of which is covered by the per-binding CI that has been green. OPERATOR ACTION, and nothing else unblocks this: open Billing & plans for the account that owns btakita/agent-loop, settle the failed payment or raise the Actions spending limit, then re-run one workflow and confirm a job actually starts. Until then treat superproject CI as ABSENT rather than passing, and do not read a green binding CI as coverage for it. Once jobs start again, re-run make check and make tmux-ci on HEAD before trusting the backlog of submodule-pointer commits pushed during the outage.
- [/] [#lzstdlibmatrixrelease] Implement Phase 5 of tasks/osoftware/plan-lazily-stdlib-conformance-rollout.md: run the capability-driven all-peer/all-channel matrix, enforce CI and mutation gates, publish coverage evidence, and release bindings inside-out only after every claimed cell is green.
- [/] [#lzjsnpmtrustedpub] [operator-action] lazily-js repository workflow/OIDC/provenance is ready, but npm rejects publication until trusted-publisher setup is completed. NEXT: authenticate npm, register `@lazily-hub/lazily-js` for `lazily-hub/lazily-js` workflow `release.yml` with no environment and allowed action `npm publish`, verify with `npm trust list @lazily-hub/lazily-js`, then prove the next version tag.
- [/] [#lzjspublish] After the operator completes npm security-key reauthentication in the Brave package-access tab, configure the GitHub Actions trusted publisher for lazily-hub/lazily-js using workflow release.yml with npm publish permission; rerun release job 30309980372 for v0.30.1 and verify the registry resolves 0.30.1.
<!-- /agent:review -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/lazily.done.md -->

<!-- completed work archived in tasks/software/lazily.done.md -->
<!-- /agent:done -->
eturn typed satisfied/timed-out/cancelled/disposed-or-unavailable outcomes; optionally prove keyed side-effect receipts while leaving receipt storage and transport policy outside the primitive. Test lost wakeups, already-satisfied state, revision races, cancellation, deadlines, disposal, and keyed receipts; document using the stdlib Timer as the deadline budget.
cate orphan line remains outside all agent-doc components at line 158. Please delete only the stray `- [ ] [#lzstdlibbarrier] ...` line in IDEA, save, and send `continue`. The completed item is already archived, so removing this duplicate is the only remaining closeout step.
ebc036
