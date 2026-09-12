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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/44ae7f3a5c03b112ffe17aa5c85d000e3d79a6e9a4875e16442309ebf1c06a1c-20260912-033614.md`*

Compacted content:
- Archived 24 response topic(s): #lzpystructcell; what shipped; #lzpypromegress; 21 more
- Prior summary/context: prior compacted content: Archived 43 response topic(s): #lzzigcanonrunner; #lzzigforkhlcpeer; #adjshashprivatefield; ...

### Re: do[#lzktblockwalk] — opus-5

> **Queue prompt:**
>
> do [#lzktblockwalk]


Done — `lazily-kt f0c012d` then `7895f21` (CI ✅), superproject `f12dda9aa`. **18 sites / 18 digests → 737 / 628**, the narrowest walk in the family and a 41× increase in the pinned population.

**The structural blocker had to go first,** exactly as the audit predicted: `declaredBlocks` was a `Map<String, JsonObject>` keyed on the fixture path, so one block per fixture was the *data structure's* ceiling and no traversal change could have widened anything. It is now keyed by site, with the walk as one pure function shared by three callers — the inventory, the clone proof, and the guard's twin.

**`#lzrunnerownjsonclone` ruled out with a test, not an argument** — and this is where the obligation I moved onto this item last cycle earned itself. kt's loader returns **text**, so every runner re-parses and hands the tracker an object from its *own* parse: structurally cpp's exact shape. The new `AssertionBlockDigestCloneTest` asserts both `Json` spellings reproduce the loader's digest and `JsonObject` equality across number-, string- and nested-bearing blocks, **and** that the digest separates `5` / `5.0` / `5e0` — without which the first assertion could be satisfied by a digest that folds the very divergence it is looking for. No divergence exists; kotlinx keeps the raw token on both sides. So the 565 unbound sites are 565 real facts, not one bug wearing 565 hats.

**Of those 565: 25 unreachable, 540 bind-pending — and Phase 2 is not done.** The 25 are the six `EXPECTED_SKIPS` fixtures, the same 25 zig, cpp and dart each landed independently from their own corpora. The 540 are sites where the block *is* reached and the runner *does* assert its keys, by direct indexing rather than through a tracker.

**I sent it back once, and that round is why this is a closeout rather than a gate.** The first commit documented the two reason classes beautifully and the guard read neither word — so a `bind-pending` entry was just an accepted excuse and the count could **grow**: a new runner area could add its sites and stay green. A slack floor wearing a reason string, which is precisely the rot this ladder exists to refuse and would have quietly undone the point of widening the walk.

Now `unreachable` is **derived from `EXPECTED_SKIPS`** in the Kotlin source, compared both ways, with a zero-parse hard error — no number anywhere, so when a `merge_cell` op lands upstream and a fixture leaves that map, its entries fail as stale instead of surviving as folklore. And `bind-pending` is an **equality at 540 failing in both directions, derived from the run** rather than from the ledger, so a ledger edit alone cannot satisfy it: above means sites appeared and the ledger absorbed them, below means a migration landed and the number must move in the same commit. An unknown reason class is an error too. Six probes, A–F, including a fixture leaving `EXPECTED_SKIPS` and an unparseable `EXPECTED_SKIPS`.

That is the distinction I care about: 540 sites are *enumerated, classified, and pinned to a number that can only be re-pinned downward* — not invisible, and not absorbable.

Phase 2 filed and appended as `#lzktbindpending`, with the per-area breakdown so it can be taken one area at a time: collections 132, reactive-graph 88, stdlib 54, ingress 42, rateshape 33, coordination 29, and twelve more. Worth flagging in advance — migrating a block onto the tracker makes the rungs *above* rung 0 start applying to it, so unconsumed and read-but-unasserted keys that were invisible will become failures. That is the point of the migration, not a regression in it.

Exchange is at 641 lines / 24 sections with the queue active — noting it, not stopping for it.
<!-- agent:boundary:ce9fbb66:lazily -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-commit-push use subagents for fan-out tasks" priority go -->
- ~~do [#lzktblockwalk]~~
- do [#lzrsblockwalk]
- do [#lzdartboundfloor]
- do [#lzdartarrayblocks]
- do [#lzorderinganchors]
- do [#lzktareapartition]
- do [#lzscenariofloormissing]
- do [#lzcireachsync]
- do [#lzfollowupqueueorder]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#lzktbindpending] [lazily-kt] PHASE 2 of #lzktblockwalk: migrate kt`s 540 bind-pending assertion-block sites onto AssertionKeys. These are NOT unreachable and NOT excused-forever — the block is reached and the runner DOES assert its keys, but by direct indexing rather than through a tracker, so rung 0 cannot see the bind and the rungs above it (unconsumed key, read-but-not-asserted, prose discharge) are all scoped to blocks a tracker bound and therefore say nothing about these 540. Before #lzktblockwalk widened the walk from 18 to 737 sites they were not excused, they were outside the rung entirely; they are now enumerated per site in scripts/conformance-unbound-blocks.txt with reason class `bind-pending`, and the guard asserts that count as an EQUALITY at 540 failing in BOTH directions and derived from the RUN rather than from the ledger — so this work cannot land without moving the number in the same commit, and cannot silently regress upward either. Per-area breakdown to pick this up incrementally rather than as one 540-site block: collections 132, reactive-graph 88, stdlib 54, ingress 42, rateshape 33, coordination 29, windowing 23, egress 22, resilience 21, service 20, temporal 16, presence 16, lossless-tree 16, message-passing 9, membership 9, protobuf 6, familysync 3, root 1. Take one area per pass, migrate its runners to `AssertionKeys` (kt`s tracker is in ConformanceFixtures.kt / AssertionKeys.kt; `noteBound` is content-keyed so a runner cannot satisfy the ledger by recognising a block NAME), delete that area`s ledger entries, re-pin the equality DOWNWARD by exactly the number removed, and verify the guard fires if you get the count wrong in either direction. Expect the migration itself to surface real gaps: the rungs above rung 0 start applying to each newly bound block, so an unconsumed or read-but-unasserted key that was invisible becomes a failure — that is the point, not a regression. Gradle proof discipline applies (JUnit XML timestamps, not task lines, since RTK strips them and a SKIPPED :test reads as passing). Repo: src/lazily-kt
- [ ] [#lzcireachsync] [lazily-rs, lazily-py, lazily-js, lazily-kt, lazily-go, lazily-cpp, lazily-cs, lazily-dart, lazily-zig] The ten copies of scripts/check-ci-reach.sh are no longer byte-identical. Before #lzgdcireach the nine all carried md5 da1444b38e93 / 570 lines — an invariant nothing enforces but which is plainly deliberate, since the script has no shared home (unlike check-assertion-ordering.py, which lives in lazily-spec and is invoked across the sibling boundary). lazily-gd 913edc8 added `make_invokes_ancestor`: a CI step running `make <ancestor>` reaches every target in that ancestor`s prerequisite closure, walked with the same `prereqs_of` the main closure uses. Without it a verbatim copy reported all six of gd`s gates unreachable, because gd`s CI is a single `make check` step. THE NO-OP PROOF IS ALREADY DONE, so this item is a port plus verification rather than a design question: the new script was run in each of the nine and produced output BYTE-IDENTICAL to the old one, exit 0 in all eighteen runs — because none of the nine invokes `make check` from a `run:` step (each runs the underlying commands per gate; the only real `run:` body in any of them containing the phrase "make check" is this guard`s own refusal text, which is the comment-vs-command trap its header warns about). So: copy gd`s version into the nine, re-run each binding`s `ci-reach` target and confirm output is unchanged, then `make check` per repo and CI. Worth deciding in the same pass whether nine-plus-one hand-synced copies should keep being hand-synced at all — the alternatives are a shared home in lazily-spec (which would make every binding`s own guard depend on the sibling checkout, a cost the current design deliberately avoids) or a guard that asserts the copies are identical. Note for whoever takes it: the file is 641 lines in gd now, and the ONLY behavioural difference is the two new functions plus one call site and the header sentence, so a diff against gd is the cheapest review. Repos: the nine above
- [ ] [#lzfollowupqueueorder] [process] Follow-up items filed by a drain cycle keep preempting the queue they were filed from, because `--backlog-add` prepends by default and the queue mirror honours that. Three consecutive cycles filed an item that became the very next head — #lzblocksitepin, then #lzexpectedkeyorder, then #lzgdorderingguard, then #lzgdcireach — each pushing #lzcppblockwalk one place further back, so the drain went depth-first into gd tooling while the three walk-widening items (cpp/kt/rs) and the three older items (#lzorderinganchors, #lzktareapartition, #lzscenariofloormissing) sat untouched. Prepending is the right DEFAULT — a follow-up from the turn that just ran is usually the most task-relevant work, which is exactly why the flag exists — but it is the wrong choice for a follow-up that is merely ADJACENT rather than more urgent, and nothing currently prompts the agent to make that judgement. Concretely: prefer `--backlog-queue-placement append` for a follow-up that is not more urgent than the existing head, and consider whether the agent-doc skill should say so at the point where it tells the agent to capture follow-up work (runbooks/respond.md, the backlog-capture rule) rather than leaving the default to decide. Worth checking whether the same asymmetry applies to `--backlog-add-after`/`--backlog-add-before`, which the queue mirror also honours. This item deliberately uses append on itself. Repo: src/agent-doc (skill/runbook text), or file against tasks/agent-doc/ if it is treated as a product change
- [ ] [#lzrsblockwalk] [lazily-rs] rs`s rung-0 walk is the same narrow shape lazily-zig retired in #lzzigblockwalk: the top-level `assertions` object plus the `assertions` object of each element of the top-level frames/scenarios/rejects arrays, and nothing else — 36 sites / 30 distinct digests of the 771/661 the same 150 opened fixtures carry under every block name at every depth, so 4.7% of sites and 4.5% of digests. The entire remainder is the four names this walk never looks at: `expected` 434 sites/359 digests, `expect` 295/266, `expect_initial` 3/3, `expect_after` 3/3 — and ZERO `assertions` blocks are missed at any depth, so the gap is purely the name set. Widen record_declared_blocks() in tests/common/mod.rs to the whole block-name set at every depth (objects only, emit-and-do-not-descend, name-preferring list labels), mirror it in the guard`s derivation so the one-walk-two-callers property holds, then bind or excuse every block it surfaces — expect runner work, not a guard edit. In lazily-zig the same widening surfaced 204 unbound digests over 239 sites across nine runner families and took a full cycle. Verify against `make corpus-blocks-report` in lazily-spec. Repo: src/lazily-rs CLONE-DIVERGENCE OBLIGATION, moved here from #lzrunnerownjsonclone because a narrow walk is the only place it can hide: when this walk widens, expect a runner/loader digest divergence among the newly surfaced unbound blocks and look for it DELIBERATELY before treating them as separate gaps. lazily-cpp widened and 71 of its 96 surfaced sites turned out to be ONE bug — its reactive-graph runner re-parsed the block and its number parse dropped the raw token, so the clone digested `5` as `#5.000000` where the loader digested `#5`, and the clone`s own comment falsely asserted content-identity. See docs/conformance.md `#lzrunnerownjsonclone` in lazily-spec 347f6fd for the rule: prefer routing the tracker at the loader`s own value over asserting equality between two parses, and if a second parse is unavoidable assert in-run that the clone`s digest equals the loader`s for a number-bearing, a string-bearing and a nested block, then perturb a number`s spelling (5 vs 5.0 vs 5e0) in a scratch corpus to confirm both sides still agree.
- [ ] [#lzdartboundfloor] [lazily-dart] scripts/check-unbound-blocks.py still carries two typed `>=` floors that #lzblockmagnitudeaudit deliberately left alone because they are different rungs: MIN_BOUND_BLOCKS=697 (blocks BOUND) and MIN_BLOCK_FIXTURES=144 (fixtures opened). MIN_BOUND_BLOCKS is now the last number in this repo a person re-pins by hand — it read 697 when pinned and the run reports 700 today, so it is already 3 slack and guarding nothing in that range, the exact drift the block magnitude just stopped. MIN_BLOCK_FIXTURES is now REDUNDANT: the site-set equality subsumes it, since the derived site set is keyed by fixture and a missing fixture takes its sites with it. Derive the bound count the same way (declared minus the excuse ledger, asserted EQUAL) or retire it in favour of the unbound-set check that already exists, and delete MIN_BLOCK_FIXTURES. Repo: src/lazily-dart
- [ ] [#lzdartarrayblocks] [lazily-dart] dart`s walk is object-valued only, so array-valued tracked keys contribute no site: the same 144 opened fixtures give 725 sites / 616 digests under dart`s rule and 737/628 when array ELEMENTS count. The gap is exactly 12 blocks and they are all in one fixture — `signaling/anti_spoof_session.json`, whose eight array-valued steps[].expect keys carry 1+2+3+1+1+1+1+2 elements. lazily-js instruments each element (which is why js derives 747 where dart derives 725); dart`s objects-only rule sees none of them, so whatever those 12 blocks assert — one of them is the anti-spoof invariant the fixture exists for — is outside rung 0 in this binding. Decide deliberately: either instrument array elements as js does, or record why an array-valued block has no bindable site here. Related and worth settling in the same pass: dart reads its block-name set out of the tracker, so a name the CORPUS grows is invisible on both the inventory and the derivation at once (a NAME-OPEN `^(assert|expect)` scan finds nothing beyond the five names today — `expected` 405, `expect` 294, `assertions` 32, `expect_initial` 3, `expect_after` 3 — but that is today`s corpus, not a guarantee). Repo: src/lazily-dart
- [ ] [#lzorderinganchors] [lazily-spec] scripts/check-assertion-ordering.py ORDERED_CHECKS anchors its ordering contract on the EXACT floor literals the bindings are now removing - regexes like 'assert accepted == 6', 'assert replayed == 12', 'assert.equal(accepted, 10)' for py/js/cs. lazily-py could not delete three of its floors this cycle and shipped them as DEAD ANCHORS, kept only to satisfy this guard, with the live constant-free assertion sitting above them. That is debt the guard created. Re-anchor on something stable that is not a value - a named helper call, or a marker the runner declares - so removing a constant does not break an unrelated ordering contract. Note the anchor must also be UNIQUE in the file (py tripped 'anchors are not unique' by quoting the literal in an explanatory comment). Repo: src/lazily-spec, then src/lazily-py to drop the dead anchors.
- [ ] [#lzktareapartition] [lazily-kt] check-conformance-coverage.sh declares REQUIRED_AREAS (27 areas) with no EXCUSED_AREAS complement, so the area list scopes the AUDIT and not the set the suite OPENS: kt opens 148 fixtures / replays 157 scenarios where deriving from the areas alone gives 147 / 151. lazily-spec's check-corpus-floors.mjs therefore declines to derive any floor for kt (it would otherwise have called an honest MIN_FIXTURES=148 unsatisfiable). lazily-cpp pairs the two arrays and its own guard enforces the exact complement, and there the derivation is exact. Add the EXCUSED_AREAS complement (or the equivalent partition assertion) so kt's floors become auditable from the corpus side. Repo: src/lazily-kt
- [ ] [#lzscenariofloormissing] [lazily-py, lazily-kt, lazily-gd] No MIN_SCENARIOS floor is declared at all - lazily-spec's check-corpus-floors.mjs reports this rather than failing, because lazily-kt deliberately retired MIN_FIXTURES for an area-partition assertion and that is a stronger invariant. But a binding with NEITHER is relying on nothing: py replays 151 scenarios and kt 157 with no positive-evidence floor under either, so a run that silently stopped replaying scenarios would report green. Derive each floor from the gate's own output, add it, and prove it exact by failing at n+1. lazily-gd is staged and may legitimately have nothing to floor yet - check before adding. Repos: src/lazily-py, src/lazily-kt, src/lazily-gd
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
