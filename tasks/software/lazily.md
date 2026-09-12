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

### Re: do[#lzrsblockwalk] — opus-5

> **Queue prompt:**
>
> do [#lzrsblockwalk]


Done — `lazily-rs ad9054d` + my `7867687` + its own `0ecfce0` (CI ✅), superproject `ee1943dfa`. **36 sites / 30 digests → 771 / 661**, and **570 of 771 bound** with 201 ledgered. That closes the last of the three narrow walks the magnitude audit found.

The shape of rs's gap was unusual and the earlier audit had it exactly right: the narrow walk was **not** missing `assertions` blocks at depth — it had all 36 — so the entire gap was the four names it never looked at.

**The clone-divergence check came back with a better mechanism than I asked for.** I asked for the kt treatment (assert the clone reproduces the loader's digest). rs went further: every `bound` line now carries the runner's own fixture and label beside the digest, so the guard can *classify every bind the loader never declared* — 397 of them, 363 below an already-emitted block and 34 from the guard's own self-tests, which fabricate blocks under borrowed fixture names. Zero divergences. And it is **reported rather than failed**, deliberately: the self-tests make a non-zero value normal, and an equality on it would be the typed constant this rung was rebuilt to remove. The contract is pinned by unit tests instead — including that the digest *separates* `5` from `5.0`, and that `5.0`/`5e0` **fold** because serde_json normalises both at parse, which cannot hide a clone divergence but can hide a corpus respelling. That distinction is finer than anything I gave it.

**The agent corrected its own headline, unprompted and in the right direction.** `ad9054d` said two of three migrated blocks hit a higher rung; `0ecfce0` corrects it to **three of three**, with each now demonstrated by removing the fix and watching the rung fire rather than asserted. So kt's 100% rate is now measured twice rather than once, which is what makes it a rate instead of an anecdote.

Two of the three are instructive about what Phase 2 will keep finding. The registers runner carried a **hand-rolled copy of rung 2** — it matched every key and panicked on an unrecognised one; right instinct, wrong place, and rung 0 still saw nothing. And `arena_blob.json`'s `expected` was consumed only by a serde deserialize into a typed struct, so it was **read, asserted, and invisible** — binding it also meant comparing against the *bound* value rather than the struct's copy, because a closure that ignores `want` satisfies the tracker while asserting nothing.

One thing I fixed myself: `AGENTS.md` claimed 571 sites bound where the gate prints 570. A documented count that disagrees with the number the gate prints is the exact drift this rung was rebuilt to remove, so it had no business sitting in the file describing that rung.

**And one genuine disagreement between bindings that I have filed rather than resolved by fiat.** rs pins only the set equality and argues that a separate typed count "can only ever disagree with it, and is the `MIN_BLOCKS = 30` defect in a new costume". kt pins *both* a count and the per-site ledger. Both arguments are coherent and only one can be right; `#lzrsbindpending` carries it so whoever takes the next pass settles it deliberately instead of copying whichever they saw first.

Phase 2 filed and appended: 201 sites, all reachable and none excusable, across reactive-graph 113, stdlib 54, ingress 28 and semtree 6.

### Re: `#lzrsbindpending` — 201 bind-pending sites to zero, and a hole in set equality that no count closes — opus-5 · 2026-09-12T02:52-04:00

> **Queue prompt:**
>
> do [#lzrsbindpending]


Done. All four areas landed, `KNOWN_UNBOUND_BLOCKS` is **empty**, and rs now reports **771 of 771 sites bound with nothing excused**.

| area | sites | what it took |
|---|---|---|
| `collections/semtree_incremental.json` | 6 | 1 real gap |
| `ingress/boundary_ingress_adapter.json` | 28 | 2 hand-rolled rung 2s deleted |
| stdlib (timer 14, timeout 18, revision_barrier 22) | 54 | whole-value equality split + key-set claim |
| reactive-graph (21 fixtures) | 113 | 1 hand-rolled rung 2 deleted |

### The 100% higher-rung rate did not generalise, and the predictor is consumption shape

I told the agent to expect 100%, measured twice. It came back **4 of 7 blocks**, and the miss is the useful part, because the split is not random — it tracks how the runner consumed the block *before* binding:

- **Per-key optional** (`if let Some(k) = block.get("k")`) — **every single one hid a real gap.** The new one this phase is the sharpest yet: `semtree_incremental`'s `expect_initial.b` was asserted by the corpus and **read by nothing at all**, because the runner only ever checked node `a`. An optional read is silent on absence *and* on a key the runner forgot to look for.
- **Exhaustive** (a whole-value `assert_eq!`, or a hand-rolled key loop that panics on an unknown key) — **none hid a gap.** stdlib's whole-value equality and all three hand-rolled rung 2s were wrong *in place, not in effect*: blind to rung 3, unreusable, and leaving rung 0 blind — but they did close the unconsumed-key hole.

So the 195-site step loops were the cheap ones and the 6-site fixture was the expensive one. Budget the remaining bindings by consumption shape, not by site count. kt's 28-of-28 was 28 optional reads; that is why it read as a law.

Three hand-rolled rung 2s found and deleted — `boundary_ingress_conformance` had **two**, and the reactive-graph engine had the one covering all 113 sites. Each iterated fixture keys and panicked on an unrecognised one: right instinct, wrong place, and rung 0 still saw nothing. All three are replaced by inverted iteration — the runner names what it implements, the tracker reports what nothing read — with object-valued keys consumed by descent, which *is* the key-set claim and is what retires `assert_delivery`'s loop.

Two traps worth keeping: splitting a whole-value `assert_eq!` per key is **strictly weaker** unless the key set is asserted beside it, because per-key comparisons cannot see a key the *run* produced that the block does not carry (stdlib now asserts both); and every closure must compare against the value the tracker hands over — at 54 sites through one helper, a single `|_|` would have been 54 fake binds.

### kt vs rs: both partly right, and I had it wrong to frame it as either/or

I said only one of them could be right. That was the wrong frame, and the agent's answer is better than my question.

rs is right that a typed count **mirroring** the current population is redundant: equal sets have equal counts, so it carries no information and adds an edit site that drifts — the `MIN_BLOCKS = 30` shape in a new costume.

But set equality alone is **satisfied by any consistent pair**, and that is a genuine hole. A commit that detaches fifty binds and writes the fifty matching ledger entries passes both directions, and the magnitude rung does not catch it either, because the sites are still *declared*, merely no longer bound. Demonstrated, not argued: dropping one `bound` line and adding its entry gets past both directions.

What closes it is not a count of what *is* excused but a **ceiling on how much may be**. That is policy rather than measurement — it does not move with the corpus, needs re-pinning only deliberately and upward, and at zero needs no maintenance at all. `MAX_LEDGERED_BLOCKS` ships at 0 in `962923a`, and the gate line now reads `771 sites BOUND, 0 ledgered of at most 0`.

Filed as `#lzledgerceiling` for the other nine ledgers, carrying the one exception the agent identified: kt may keep a number **if** its pending set is matched by *pattern* rather than enumerated per site, since a pattern can widen silently and set equality cannot see that. An enumerated list cannot — growing it is the most visible diff there is. Check which kt has before removing anything.

### Verification

`make check` exit 0 captured explicitly, `grep -c "result: FAILED"` → 0. Three intermediate reds fixed rather than waived (a dead `expect_field_i64`, an unused `scenario_index`, `cargo fmt`). Probes ran per area in both directions, and the backward direction — the one nothing else catches — got a free live proof: the first full ingress run after binding, with all 28 entries still present, failed with **exactly 28 stale entries and zero forward failures**. The ceiling has its own probe: a detached bind plus its matching entry passes both directions and is refused only by the ceiling.

Six commits, `5dbaa6c` → `95fd125`, in sync with `origin/main`; superproject pointer `a5f99415c`. **CI green on `84b4dea`, `242cd2a`, and `95fd125`** — all four jobs on each, with the final log carrying the gate line.

Three things stated plainly rather than smoothed over:

- **CI ran on 3 of the 6 heads.** `5dbaa6c`, `2c4ba79`, and `962923a` are ancestors of green heads, so their content is validated, but those individual tree states were verified only locally. I am accepting that rather than spending three more runs: the content is covered and the head is green.
- **The `unreachable` class still has no entries** and its failure path is unprobed — now doubly so, since a ceiling of 0 would refuse such an entry before the class logic ran. Filed as `#lzunreachableunprobed`, because an enforced-but-unexercised branch reads as coverage and is not.
- The `5.0`/`5e0` digest fold and the untested float rendering in the guard's Python twin carry over unchanged from `#lzrsblockwalk`.

### `#lzfollowupqueueorder` now has four witnesses

`#lzrsbindpending` was filed to backlog at the end of last cycle and arrived as this cycle's head, ahead of `#lzdartboundfloor` and the six items behind it. Same mechanism as the previous three: the closeout that filed it hit the retained-write deferral, so the `--backlog-queue-placement append` path never ran, and the next preflight's backlog→queue sync reinstated it in backlog order, which is prepend.

Separately, this head took three preflight admissions (`cycle-1789191617647`, `cycle-1789194895273`, `cycle-1789200...`) while one dispatch was in flight. My re-entries came through the Skill tool — a compaction resume, then two Stop-hook blocks — and a Skill-tool re-entry does not claim the `claude_loop` drain-owner lease the way a submitted `/loop agent-doc <FILE>` prompt does, so the supervisor idle-queue watch injected its own trigger each time. Re-arming through `ScheduleWakeup` produces a real submitted prompt and does claim the lease, which is the shape I have settled on.
<!-- agent:boundary:ce9fbb66:lazily -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-commit-push use subagents for fan-out tasks" priority go -->
- ~~do [#lzrsbindpending]~~
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
- [ ] [#lzledgerceiling] [lazily-kt, lazily-zig, lazily-py, lazily-js, lazily-cs, lazily-dart, lazily-gd, lazily-cpp, lazily-go] rs proved set equality alone has a hole that no count closes: a commit that detaches N binds and writes the N matching ledger entries passes the equality in BOTH directions, and the magnitude rung does not see it either because the sites are still DECLARED, just no longer bound. Demonstrated by dropping one `bound` line and adding its entry. The guard that catches it is not a count of what IS excused but a CEILING on how much MAY be — policy, not measurement, so it does not move with the corpus and never needs re-pinning except deliberately and upward. rs shipped `MAX_LEDGERED_BLOCKS` defaulting to 0 in 962923a. Propagate a ceiling to every other binding that carries a block ledger, set to that binding's CURRENT ledgered count so it is a no-op on landing and a fail on any growth, then drive it down as the pending sets shrink. TWO SPECIFIC COROLLARIES. (1) lazily-kt must ALSO drop its typed count beside the set equality: a count that MIRRORS the current population adds no information (equal sets have equal counts) and adds an edit site that drifts — the `MIN_BLOCKS = 30` shape in a new costume. The ONE case kt may keep a number is if its pending set is matched by PATTERN rather than enumerated per site, because a pattern can widen silently and set equality cannot see that; an enumerated list cannot, since growing it is the most visible diff there is. Check which kt has before removing anything. (2) Budget the remaining bind-pending work by CONSUMPTION SHAPE, not site count — see #lzrsbindpending's measured 4-of-7.
- [ ] [#lzunreachableunprobed] [lazily-rs] The `unreachable` ledger class in rs's block guard has NEVER had an entry, so its failure path has never been exercised, and with `MAX_LEDGERED_BLOCKS` now 0 the ceiling would refuse such an entry before the class logic ran. Either probe the class against a scratch corpus that legitimately carries an unreachable site (raising the ceiling for that probe only), or delete the class and its vocabulary entry as dead code. Do not leave an enforced-but-unexercised branch: it reads as coverage and is not. Repo: src/lazily-rs
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
