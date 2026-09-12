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

### Re: `#lzledgerceiling` — nine ledgers, nine ceilings, and kt proving me half wrong — opus-5 · 2026-09-12T03:19-04:00

> **Queue prompt:**
>
> do [#lzledgerceiling]


Done in all nine bindings. Every commit CI-green on its exact SHA, verified independently rather than taken on report: `py 34e9930 · js 3774a7c · kt aa84b2c · go 2eaf2a2 · cpp 2c9bb0c · zig f898f4d · cs 2d4125e · dart cfd80e7 · gd a4779e7`, superproject pointers in `fa3767dcf`.

| repo | ledger | ceiling | mirroring count |
|---|---|---|---|
| kt | 537 (512 bind-pending + 25 unreachable) | 537 | **`MIN_BIND_PENDING` deleted** |
| cpp | 25 | 25 | none |
| zig | 25 | 25 | none |
| dart | **25**, not 24 | 25 | none |
| py | **25** (in `tests/`, not `scripts/`) | 25 | **`MIN_FIXTURES=145` deleted** |
| js | 0 | 0 | none |
| cs | 0 | 0 | none |
| gd | 0 | 0 | none (`FLOOR = 8` kept, correctly) |
| go | 0 (`unboundBlockExcuses`, in `_test.go`) | 0 | none |

**Two of my nine baselines were wrong, both caught by the agents rather than by me.** dart is 25, not 24 — my method counted quoted strings, which is the right unit for a flat bash array but not for dart's six tuples expanding per index; the guard's own `(25 excused)` line and `len(expand_excuses())` both say 25, and the outer count of 6 would have been the wrong unit entirely. And I reported py as having no ledger because I only grepped `scripts/`; it has 25 entries in `tests/conformance_assert.py`. go's was likewise not in the shell script but in `conformance_unbound_block_test.go`.

### The hole is real, and it reaches through `make check`

Every one of the nine ran the attack rather than reasoning about it, and it went green in all nine: detach one bind, add its matching excuse, and both directions of the set equality pass with the magnitude rung unmoved — because the site is still *declared*. The prints that matter:

- **gd** took it through **full `make check`**: exit 0, zero FAILED, all gates OK across 35 tests, load-graph, coverage, ordering, ci-reach. Not just the guard — the repo's whole verification.
- **cs**: the test suite itself stayed green at **325 + 7 passed** with the bind gone. Nothing in the tests notices.
- **cpp**: `ctest` **100% of 64 passed**, using the sharpest mechanism of the nine — it made the runner record a *wrong* digest rather than skip the call, reproducing `#lzrunnerownjsonclone` exactly, so the block stays declared and every key still asserts.
- In all nine, `MAX_LEDGERED_BLOCKS=N+1` goes green again, which is what proves the ceiling is the *only* refusal and not a side effect of the probe edit.

`lazily-spec/conformance` was never written by any of the nine. Seven of them found they did not need to: detaching a *bind* is a manifest or runner operation, not a corpus one — a simplification worth remembering, since the scratch-copy dance exists to protect the shared corpus and this attack never touches it.

### kt proved me half wrong, and it is the most useful result here

I sent kt to drop its typed count on rs's argument. kt ran the probe against **pre-change** code first and `MIN_BIND_PENDING` caught it: `513 ... the pin is 512. The ledger has GROWN.`

So **kt was never vulnerable to the hole**, and this change is lateral there rather than a fix. The reason is a distinction neither rs nor I drew: rs's argument holds against a count *derived* from the ledger, which is genuinely redundant. `MIN_BIND_PENDING` was a typed **pin**, and a pin is two-sided — it failed on growth *and* on shrink. `MAX_LEDGERED_BLOCKS` uses `>`, so it keeps the growth half and discards the shrink half. Measured: binding a site and deleting its entry now exits 0 at `536 ledgered of at most 537`, where the old pin failed and demanded 511.

What the ceiling does buy in kt is real but narrower than I implied: it bounds **both** classes, where the count reached only the 512 reachable ones — the 25 `unreachable` sites were held by a derived equality with no number anywhere, so growth in that class was unbounded. kt could not *demonstrate* that marginal coverage, only argue it, and said so: every reclassification constructible from the current state also moves the `bind-pending` population, so the old count notices too, and the separating case needs a genuinely new skipped fixture in the corpus.

That leaves a live design question rather than a settled answer, filed as `#lzledgerratchet`: make the ceiling an **equality** — a ratchet lowered in each migration commit, which is what a typed pin already was — or keep `>` and accept accumulating slack, saying so in each guard so nobody re-adds a pin thinking it was an oversight. What I will not do is leave the ten bindings split on it, since that split is exactly what `#lzrsbindpending` was filed to resolve and this item has now reproduced it one level up.

### The one legitimate keep-case, and the one real deletion

**gd kept `FLOOR = 8` and was right to.** That number floors how many GDScript source files a directory **walk** examined — not how many sites are excused. A walk can silently narrow through a moved directory or a bad root, in a way no set equality sees. That is precisely the pattern-matched exception, and it is a different population from the ledger. Actual scan: 19 sources.

**py deleted a genuine mirror and proved it by running it.** `MIN_FIXTURES=145` was implied by the script's own loops — 156 corpus fixtures minus 11 excused is exactly 145, the pin's own value. Dropping a fixture takes `covered` to 144, but the run dies earlier at *"canonical fixture was NOT opened by the suite"*, before the floor is ever evaluated. The pin's comment block was itself a drift ledger: 132 → 139 → 141 → 145. It kept the one non-implied claim, `covered -eq 0`, and proved *that* guard fires using a scratch corpus of only the excused fixtures.

### Three things found on the way, all filed

- **`#lzzigenvsecretleak`, and it is the serious one.** zig's `failed command:` lines dump the entire environment, and I verified by presence check that this machine exports `CARGO_REGISTRY_TOKEN` (35 chars) and `NPM_TOKEN` (40 chars). So an ordinary green `make test` in a lazily-zig checkout prints two live publish credentials in cleartext. The agent deleted three captured logs that contained the token. The root cause is not zig: tokens are exported into every shell instead of being substituted at the point of use, which bypasses the documented `passage` pattern.
- **`#lzgotypedfloors`** — go's `MIN_FIXTURES=147` / `MIN_SCENARIOS=151` mirror the *coverage* ledger, which is why this item left them alone. They cannot just be deleted: they are the only thing there that can see a **shrinking corpus**, since `total` reads from the overridable `SPEC_DIR`. rs already did the derived-equality conversion under `#lzrstypedfloors`. Carries py's trap with it: `check-corpus-floors.mjs` greps `scripts/` for the literal pin spelling, so quoting a retired pin in a *comment* keeps the upstream audit comparing a floor that no longer runs — the `#lzorderinganchors` failure mode again.
- **`#lzgoasyncflake`** — an intermittent `AsyncContext/churn_returns_to_baseline.json#3:observed_count` divergence that did not reproduce in two reruns with control green 3/3. Flagged, not chased. An intermittent divergence in an async runner is the shape that hides a real race behind a retry.

### Stated plainly

**kt's `unreachable`-class coverage is argued, not demonstrated** — as is rs's `unreachable` class generally, which still has no entries and an unprobed failure path (`#lzunreachableunprobed`). **py's `signal.py` is newer than its mypyc `.so`** in this checkout (2026-09-10 source against a 2026-07-31 build), so the suite there runs stale signal code; the agent correctly declined to `make compile` since nothing it touched is in the compiled extension and another pane may own that build state. **cs edited `AGENTS.md` around another pane's uncommitted tsift block** by hand-building the blob as `HEAD` plus its own hunk and bare-committing, rather than `--only`, which would have swept that change in — the right call, and verified after.
### Re: `#lzledgerratchet` — the ceiling self-disables, measured ten times — opus-5 · 2026-09-12T03:58-04:00

> **Queue prompt:**
>
> do [#lzledgerratchet]


Decided and landed in all ten bindings. `EXPECTED_LEDGERED_BLOCKS`, exact equality in both directions, no alias — the retired name returns **zero files** family-wide. Values unchanged, so every landing was a no-op: `kt 537 · cpp/zig/dart/py 25 · rs/js/cs/gd/go 0`. All CI green on their exact SHAs: `rs 959c004 · py 30ad1ab · js 293b200 · kt b40c8d7 · go b420a30 · cpp 4b5910c · zig eb738c8 · cs 998dafa · dart 23e397f · gd a2dd7dc`, pointers in `8dc0effe4`.

### The decision, and the better framing rs gave it

I argued a `<=` ceiling is wrong because it starts at zero slack and accumulates slack with every migration, converging on the `MIN_BLOCKS = 30` defect it replaced. rs — the author of the position I was sharpening — agreed and then improved the cut:

> The distinguishing property is not policy-vs-measurement; it is **fails-when-stale vs passes-when-stale**.

That is better than what I wrote, and it explains why py's deleted `MIN_FIXTURES=145` and kt's deleted `MIN_BIND_PENDING` land on opposite sides despite both being typed numbers: the first was implied by a derivation in its own script and so could never fail, while the second could. rs also caught that its own comment was self-refuting — it invited a raise ("Raise it ONLY for a genuinely unbindable block"), and that invitation is precisely what arms the self-disabling path. At a pin of 0 the ceiling was safe only because a ledger cannot go below zero. **The policy framing was doing none of the work; zero was.**

### Measured, not argued — ten times, both operators, same world state

Every binding ran the two-step with the old and new operators against byte-identical evidence. The flaw reproduces everywhere:

- **py put the stale pin in the file**, the way a real migration commit does, and the attack ran **fully green**: exit 0, zero `result: FAILED`, 1026 passed, guard printing `729 site(s) / 620 distinct digest(s) … 26/26 of the ceiling in use`. One migration's slack bought the attack outright. Supplying the same stale pin by *env* was still caught by a self-test — so only the file edit reproduces the real hazard, and an env-based probe would have under-reported it.
- **gd and cs used a second site** so the two-step is genuine: migrate site A, then attack with site B. A single-site probe cannot show this, because migrating the only target removes the thing you need to attack with.
- **cpp built the faithful migration** — an `LZCPP_ATTACH_BIND` hook emitting an extra `@block_bound` line, so a ledgered site actually *starts* binding rather than merely losing its entry.
- **kt reproduced the exact case I quoted at it**: `>` exits 0 at `536 ledgered of at most 537`; the equality exits 1 naming the pin to lower.
- **go stated the mechanism most cleanly:** under the equality, step 1 is already red, so the stale pin never survives to admit step 2. The growth check does not need to catch the laundering if the shrink check makes its precondition unreachable.
- **js proved the equality is fail-closed by construction** where `>` was not: with the validation block deleted in a scratch copy, a malformed pin still exits 1 on `0 !== NaN`, while the same deletion under `>` exits 0 with a green OK line.

### Three defects found inside the work itself

- **cs's own prior commit accepted `-1` as a pin**, and three of four malformed values died on an unhandled `ValueError` traceback rather than a named error.
- **py had a false green in its self-tests**: one planting test asserted with `any(...)` over the report and let a stray pin line slip past. Four others failed outright once the operator became exact, each reading as `GREW to 1 against a pin of 0`. Fixed in the fixture *and* the plants — without both halves they fail in both directions.
- **go's site-list cap could have hidden entries from the count.** I told them to cap the displayed list; go added `TestLedgerSiteListIsCapped` asserting the reported *count* stays the full population. That failure mode was mine to spot and I did not.

### rs's caveat, which belongs with `#lzktbindpending`

> The equality costs one deliberate edit per migration … That cost is real and I now think it is the *feature* — but it is only cheap because the pin is 0 and the ledger is empty. A binding sitting at a pin of 200 mid-migration pays it on every commit. rs is not in that state; kt may be.

Correct, and worth carrying rather than discovering later: kt has 512 `bind-pending` sites to work through, so its pin rides every migration batch. That is the intended friction, but it is friction rs does not pay and kt will.

### Reconciling the drift rs declined to touch

rs flagged two comment-drift items in its guard. It fixed one (prose claiming "113 entries, all `bind-pending`" directly above an empty array) and deliberately left the other: its comment says "all THREE blocks bound here needed a fix above rung 0" while AGENTS.md says 7 blocks, 4 real fixes. It said rewriting migration history it had not run felt worse than leaving the contradiction visible.

That was the right instinct, and the two statements are **both true at different scopes** — the records are in this session. `#lzrsblockwalk` bound 3 blocks and all 3 needed higher-rung fixes (3 of 3). `#lzrsbindpending` measured 4 of 7 cumulatively: those same 3, plus `semtree_incremental`'s `expect_initial.b`, the key asserted by the corpus and read by nothing. So 3-of-3 describes the first pass and 4-of-7 the total. No defect, and I am not filing one.

### Stated plainly

The one new item is `#lzpinparsestrict`: the ten parsers disagree about what counts as a number, and rs's stricter rule is right — `int("1_0")` is 10, `int(" 7 ")` is 7, and `"٣".isdigit()` is True. All ten are fail-closed on obvious garbage, so this is an inconsistency rather than a hole, but the pin is now the rung a consistent pair cannot satisfy, which makes its parse load-bearing in a way it was not as a ceiling.

Also unchanged and still true: gd's `FLOOR = 8` stays (it guards a directory *walk*, which no equality sees), dart's `MIN_BOUND_BLOCKS`/`MIN_BLOCK_FIXTURES` are byte-identical for `#lzdartboundfloor`, go's `MIN_FIXTURES`/`MIN_SCENARIOS` are untouched for `#lzgotypedfloors`, and `#lzgoasyncflake` did not reproduce across four verbose runs plus a `-race` run. Exchange is at 169+ lines; the queue is active, so I am noting the size rather than stalling to compact.

### On `#lzzigenvsecretleak` — I had the at-rest source wrong, and so did zig's agent

zig reported "three live PyPI tokens in cleartext" in the superproject `.env`. **That is wrong, and I checked rather than relaying it.** Every credential key in that file is a `$(passage ...)` reference, and the file is untracked and gitignored — correct hygiene, not a secret. Classifying a passage reference as a literal is exactly the trap.

The real chain is one step later: `.mise.toml` carries `[env] _.source = ".env"`, which **resolves** those substitutions at shell startup, so every process's ambient environment holds the cleartext — which is why a presence check sees `CARGO_REGISTRY_TOKEN` at 35 chars. zig's `failed command:` lines then print the resolved values, **on green runs** (six such lines at exit 0 with zero FAILED), and a caller-side `env -u` scrub of 20 token-shaped variables was *defeated*.

So the fix is to stop resolving publish tokens into the ambient environment, not to move them out of `.env`. The item is updated accordingly. **The rotation recommendation stands for a different reason:** the resolved values passed through a subagent's tool output while it diagnosed this, which puts them in a transcript that no log shredding reaches. `PYPI_PUBLISH_TOKEN`, `CARGO_REGISTRY_TOKEN` and `NPM_TOKEN` want rotating, and that needs your credentials, so it is genuinely yours rather than something I can close.

### Re: `#lzpinparsestrict` — one author, and two of the parsers were worse than I said — opus-5 · 2026-09-12T04:50-04:00

> **Queue prompt:**
>
> do [#lzpinparsestrict]


Done in all ten bindings, ten commits, all CI green on their exact SHAs, all verified against the reported hashes: `rs d62fb11 · py c4a5019 · js 1b9caba · kt 4458b3d · go d497195 · cpp 7ced215 · zig 56673bf · cs 2d8ccd5 · dart abe57c4 · gd 66e77cd`, pointers in `8af66b58a`.

The rule as landed: a non-empty run of ASCII `0`-`9`, validated **before** any parse runs; unset takes the committed default, explicitly empty is a rejection; leading zeros fine, `0` still valid.

### One agent, not ten — and that is what found the real bugs

I ran this as a single agent across all ten repos rather than one per repo, because ten independent authors is what *created* this defect: the family's contract had become whichever parser was loosest. Fixing it with ten more authors risked the same divergence.

It paid off in a way parallel agents could not have managed. The agent noticed that **Go's `\d` is ASCII-only while Python's `\d` is Unicode**, and that dart had built its reader on the wrong half of that asymmetry. That is a cross-language observation; no per-repo author sees it.

### I mischaracterised two of the ten, and the corrections matter

In filing this I described dart as using "a regex" and js as using "`Number()` with an explicit NaN/empty/negative rejection", implying both were already reasonable. Both were wrong:

- **dart** used `re.fullmatch(r"\d+", raw.strip())`, and Python's `\d` matches `٣`, so the non-ASCII numeral passed. Worse, it then called `int()` on the **unstripped** value — validation and parse were reading different strings.
- **js** used `Number()`, which was the loosest of the ten: `" 7 "` → 7, `+1` → 1, `1.0` → 1 with `isInteger` agreeing, and **`0x19` → 25**. A hex literal silently enforcing an unrelated number.

Two findings I would call more than cosmetic, and both were latent in commits from the previous two cycles:

- **cpp accepted `-1`.** Under a `<=` ceiling a negative pin merely fails every non-empty ledger. Under an exact equality it makes the rung **unsatisfiable** — no ledger size can ever match it. The ratchet change converted a harmless value into a wedge.
- **go and py silently turned an explicitly empty override into the committed default.** `os.Getenv` plus `TrimSpace` in go, `not raw.strip()` in py. So `EXPECTED_LEDGERED_BLOCKS= make check` read as "use the default" rather than as an error.

### Two tests were asserting the defect

Both repos had encoded the wrong behaviour as an expectation, which is why it survived:

- **py's `test_an_empty_size_pin_override_is_the_committed_pin`** asserted exactly what this item removes. Replaced, and **mutation-checked**: reverting the reader to `int(raw.strip())` with the empty fall-through turns 7 of 20 selected tests red, so the replacements are not vacuous.
- **go's test labelled `t.Setenv(env, "")` as "unset"** and asserted `" 3 "` parses as 3. Unset now uses `os.Unsetenv` — after a `t.Setenv` to register the restore, since `t.Setenv` cannot express absence.

### Where a language forced a divergence, stated rather than hidden

- **go** has no `strip(charset)`, so the class is byte-wise `isBareASCIIDigits` — deliberately not `unicode.IsDigit`, which is true for `٣`.
- **kt** uses bash `case ''|*[!0123456789]*`, enumerating the ten digits rather than writing `0-9`, because a bracket **range** is resolved by locale collation and only `LC_COLLATE=C` guarantees it means the ASCII ten. It behaved correctly under `en_US.UTF-8` already; the enumeration removes the dependency.
- **cpp** kept its deferred-failure shape (`failed = True`, report the remaining rungs, then exit) rather than exiting inline.
- **js** uses `/^[0-9]+$/` rather than `\d`, same reason as go.

Accept-set confirmed everywhere: the committed value and its leading-zero form pass, and a wrong-but-well-formed value like `26` or `538` parses and then fails on the **equality** with a visibly different message from the parse rejection. That distinction was the point — a bad pin and a bad *number* should not look alike. All ten: unset variable, `make check` exit 0, zero `result: FAILED`.

### Stated plainly

**My severity framing was right and I am not upgrading it.** Every parser was fail-closed on obvious garbage, and a wrong pin fails the equality loudly, so this was an inconsistency rather than an open hole — with the two exceptions above, which came from the ratchet work itself rather than from anything older. No corpus was touched, no superproject pointers were bumped by the agent, and zig's build output was never written to a file, per the live exposure in `#lzzigenvsecretleak`.

Exchange is at 228 lines across 5 response sections and the queue is active, so I am noting the size rather than pausing to compact.

### Re: `#lzzigenvsecretleak` — the leak does not reproduce, and I should not have relayed it — opus-5 · 2026-09-12T05:01-04:00

> **Queue prompt:**
>
> do [#lzzigenvsecretleak]


I filed this item from a subagent's report without testing the claim. The claim is wrong, and the correction is the substance of this cycle.

### What I told you, and what is actually true

I said a green `make check` in lazily-zig prints the ambient environment including live publish tokens, on six `failed command:` lines. Measured on the real repo, real toolchain, green run: **exit 0, zero `result: FAILED`, exactly six `failed command:` lines — and those six lines carry zero environment variables.** Not even `LAZILY_CONFORMANCE_MANIFEST`. Zero token-shaped values anywhere in the log, which I then shredded.

The six lines are real. The environment in them is not.

### The mechanism, read rather than guessed

`std.Build.Step.allocPrintCmd` prints an env entry **only when the child value differs from, or is absent in, the parent**:

```zig
if (env.parent.get(key)) |process_value| {
    if (std.mem.eql(u8, value, process_value)) continue;
}
```

And `setEnvironmentVariable` clones the step's map from zig's *own* process environment (`Run.zig:584-599`). So an ambient variable is byte-identical on both sides and always skipped. It is structurally incapable of printing an inherited token. Verified across all three toolchains this repo gates on: 0.17.0-dev empirically with decoys, 0.15.2 at `Step.zig:805-812` and 0.16.0 at `Step.zig:788-794` statically.

The decoy test is the one I should have run first: two token-shaped fakes in the parent environment, one child-only variable set on the step, deliberate failure. Output printed the child-only variable and **neither decoy**.

### The hazard is real but inverted, and that is the fix

What zig discloses is a **child-only** variable. So the rule worth pinning is the opposite of a scrub: never set a secret-bearing variable on a Run step. A path is safe; a token is not. That is now a comment at the call site in `build.zig` — `ec9bd33`, CI green, pointer `a87928c99` — where someone editing that block will read it, rather than in a doc they would not open.

It carries the corollary, because it is genuinely counterintuitive: **`env -u TOKEN make check` manufactures the disclosure.** The scrub removes the variable from the parent while anything re-injecting it into the child makes it child-only — which is precisely the shape zig prints. The agent's mitigation attempt was the most dangerous thing in the sequence, and it reported the attempt as evidence of the problem.

### The two things that survive

- **`#lzambienttokenresolve`** — verified and yours. `.env` holds `$(passage ...)` references and is untracked and gitignored, which is correct; the subagent misread those refs as literal tokens, and I corrected that last cycle. But `.mise.toml` resolves them at shell startup, so `CARGO_REGISTRY_TOKEN` (35 chars) and `NPM_TOKEN` (40 chars) are live in the ambient environment right now — verified by presence check, never printed. Nothing here was found printing them. It means every tool in the tree is one env-dumping code path away, and build systems have many. The fix is substitution at the point of use, and it is yours because changing `.mise.toml` alters the environment of every session started from this tree — thirty peers were live, several mid-build.
- **Rotation still stands, for one reason only.** Not because a build printed them: because the resolved values passed through a subagent's tool output while it diagnosed this, which puts them in that transcript permanently. Shredding a log does not reach a transcript. `CARGO_REGISTRY_TOKEN`, `NPM_TOKEN`, `PYPI_PUBLISH_TOKEN`.

Also filed `#lzzigemptyparentdump` as low: zig **0.15.2**, one of the two toolchains CI gates on, has a silent full-dump path the others lack — `Step.zig:806` falls back to an *empty* parent map on `getEnvMap` failure, and an empty parent makes every child key print. Reaching it needs an allocation failure, so it is latent; it is worth holding because the fallback is silent and the consequence is the whole environment.

### What I take from this

The report was specific, confident, and came with a count that turned out to be correct (six lines) attached to a claim that was not (what those lines contain). A precise detail next to a wrong conclusion is more persuasive than either alone, and I passed it to you as fact and wrote it into operator memory before testing a single thing. Memory is now corrected, including the index line. The decoy test cost about two minutes.

The operator-memory note I wrote in the same breath was wrong in the same way, and it is worth saying that a false fact recorded is worse than no fact, because the next session inherits it as settled.

### Re: `#lzambienttokenresolve` — the ambient tokens serve nothing automated — opus-5 · 2026-09-12T05:06-04:00

Gated, not closed, because both halves need you. But the blocking question is now answered, so this is a decision rather than an investigation.

### What depends on those ambient tokens: nothing automated

I audited every reference in the tree. All of them are under `.github/`, and most of the CI paths hold no long-lived token at all:

| path | how it authenticates |
|---|---|
| `lazily-js/.github/workflows/release.yml` | OIDC trusted publishing — its own comment: *"no long-lived NPM_TOKEN"* |
| `tsift/.github/workflows/release.yml` | OIDC — *"no CARGO_REGISTRY_TOKEN secret"*; exchanges the Actions OIDC token for a short-lived one at run time |
| `lazily-py/.github/workflows/wheels.yml` | the GitHub repo secret `secrets.PYPI_PUBLISH_TOKEN` |
| any Makefile, `.mk`, or other local tooling | **zero hits** |

So the resolved copies in every shell's environment are **risk with no function**. That is a materially easier decision than the one I described when I filed this: it is not "change a working credential path", it is "remove dead credentials from every process".

**The one real local consumer** is the case that py wheels comment names itself — *"the same token Claude/Codex use for `twine upload`"*. Ad-hoc manual publishing. Which is exactly what point-of-use substitution replaces: `PYPI_PUBLISH_TOKEN=$(passage ...) twine upload`.

### Why I am still not doing it

`.env` is your personal, untracked, gitignored file, and the failure mode of getting this wrong is a publish path that looks fine until the next release. My rule for anything hard to reverse or outward-facing is to confirm first, and editing someone's credential environment qualifies even when the audit says it is safe. The `$(passage ...)` references should stay either way — they are documentation, not secrets.

I will also correct something I said when filing this. I claimed the change would perturb the thirty live peer sessions. **That was wrong**: mise resolves at shell startup, so already-running sessions keep the environment they have; the change would affect shells started afterwards. The reason to leave it to you is the credential path, not disruption.

### Rotation, unchanged and the more urgent half

`CARGO_REGISTRY_TOKEN`, `NPM_TOKEN`, `PYPI_PUBLISH_TOKEN`. Not because anything printed them — the zig leak was tested and disproven last cycle — but because the resolved values passed through a subagent's tool output while it diagnosed that non-leak. That is in the transcript permanently, and shredding a log does not reach it. crates.io has no Revoke button, only Regenerate.

Given the audit above, rotation is also cheaper than it looks: two of the three CI paths do not consume a long-lived token at all, so regenerating them breaks nothing automated. Only the `PYPI_PUBLISH_TOKEN` repo secret needs updating alongside.

### What unblocks this

One decision from you: whether to stop `mise` resolving publish tokens into the ambient environment of this tree, given nothing automated reads them. If yes, the edit is two lines in your `.env` and the substitution goes at the point of use. Rotation is independent and can happen whenever you are at a terminal.

Recorded in operator memory as `reference_ambient_publish_tokens_serve_nothing_automated.md`, so a future session does not re-run this audit.

### Note on the memory index

Adding that entry pushed `MEMORY.md` over its 24.4KB load limit, which silently truncates entries rather than failing. I trimmed two over-long index lines back under — both recovery entries whose detail I verified is present in their own topic files first (22 and 5 matching lines respectively). The index is at 24,886 bytes. Worth knowing that it is close to the ceiling: the next few additions will each need a trim.

### Re: `#lzzigemptyparentdump` — unreachable here, and my filing had 0.15.2's role wrong — opus-5 · 2026-09-12T05:10-04:00

> **Queue prompt:**
>
> do [#lzzigemptyparentdump]


Closed with no code change. The path is real in upstream zig 0.15.2, but it cannot execute in this repo, and the reason corrects something I wrote when filing it.

### Three facts, each checked

**1. The fallback exists only in 0.15.2.** `Step.zig:806` reads `std.process.getEnvMap(arena) catch std.process.EnvMap.init(arena)`, and an empty parent map makes every child key print. Both **0.16.0** and **0.17.0-dev** have no such call — they pass the graph's own map instead of re-reading the environment. Upstream fixed it; nothing to report.

**2. Its only failure mode is OOM.** `GetEnvMapError` is `{ OutOfMemory, <WASI-only> }`, so on Linux the catch branch is an allocation failure in the build runner.

**3. The decisive one: 0.15.2 never runs `zig build` in this repo.** `build.zig.zon` declares `.minimum_zig_version = "0.16.0"`, and CI installs 0.15.2 solely to record `ZIG_FMT_0_15_2` for `scripts/check-fmt.sh`. That variable is consumed by nothing else — one hit in the workflow, two in the script's own comments. The `zig build test` and `zig build interop-peer-check` steps run under 0.16.0, which was installed second.

`allocPrintCmd` is build-runner code. `zig fmt` never constructs a Run step, so the 0.15.2 binary never reaches it. The path is not merely OOM-gated here, it is unreachable.

### The correction

I filed this saying 0.15.2 is *"one of the two toolchains this repo gates CI on"*, which implied it builds. It gates **formatting**. That is the difference between a latent hazard and a non-hazard, and I asserted the wrong one from a version number in a CI matrix without checking what the version was for.

### What stands

The mitigation from the previous cycle is the one that matters and it is already landed (`ec9bd33`): never set a secret-bearing variable on a Run step, because zig prints child-only variables. That rule holds on every toolchain and does not depend on this analysis.

Worth stating what it does *not* cover, since I checked: on the 0.15.2 fallback the printed map is the **child** map, which `setEnvironmentVariable` clones from the ambient environment — so that path would dump everything regardless of the comment's advice. The only fix that would cover it is an explicit minimal env map for those Run steps, which would strip the test binaries of `PATH`, `HOME` and locale. Given the path is unreachable here, that trade is not worth making, and I am recording the reasoning rather than leaving a future reader to re-derive it.

### Re: `#lzgotypedfloors` — the floors go, and rs was quoting its retired ones — opus-5 · 2026-09-12T05:48-04:00

> **Queue prompt:**
>
> do [#lzgotypedfloors]


Done. `go a5d44f8` and `rs 3bf4d73`, both CI green, pointers in `cd2f158b7`.

### I filed this with the wrong justification, and it was checked rather than trusted

I wrote that go's `MIN_FIXTURES=147` and `MIN_SCENARIOS=151` "are the only thing in that script that can see a SHRINKING corpus". They are not. `lazily-spec/corpus-counts.json` pins `"fixtures": 156` plus per-fixture scenario counts, and `check-corpus-floors.mjs:447` asserts equality against it across all ten bindings.

I asked go to verify that rather than take my word, because the case for deleting the floors rested entirely on it. It did, against a scratch corpus with one fixture removed:

```
ERROR: the corpus carries 155 fixtures; corpus-counts.json pins 156.
corpus floors FAILED: 7 problem(s)     exit=1
```

One edit site, an exact equality, in the repo the corpus lives in. The ten hand-typed floors were the weaker half of that pair.

**What the conversion actually buys** is what rs's own comment says, and it is not shrink detection: *"a duplicated excuse entry passes both directions while making the count disagree."* Arithmetic. Which is why the work had three parts rather than one — derived equalities with `-ne`, duplicate checks **ordered ahead** of them, and both zero-guards kept (plus two new `expectation <= 0` guards).

The probe that justifies the ordering is the good one: with the duplicate check neutralised, the equality fires with exactly the misleading message the ordering exists to avoid — *"147 distinct fixtures were OPENED but the corpus minus KNOWN_UNCOVERED derives only 146 … not the same tree"*. And with the directional loops neutralised where they report first, the equality's *unique reachable* contribution really is the arithmetic case. That is the difference between believing rs's comment and confirming it.

One detail worth keeping: the scenario expectation walks the **corpus listing** minus `KNOWN_UNCOVERED`, not `SCENARIO_TOTAL`. The latter walks only manifest-opened fixtures, so a detached recorder drags manifest, total and replayed down together — the derived-from-the-manifest trap, avoided.

### The finding I fixed rather than filed

go noticed that **lazily-rs was in the trap py had already hit.** rs's `#lzrstypedfloors` comments quoted both retired constants in their greppable assignment forms, at `check-conformance-coverage.sh:543` and `:827`. `check-corpus-floors.mjs` greps `scripts/` for that literal shape, so it was still deriving and comparing floors of **150 and 166** — numbers nothing in rs runs. It passed only because the derivation happened to agree, and would have broken the moment rs's `KNOWN_UNCOVERED` changed.

The tell was visible in the audit's own output and I had read past it: the NOTE listed py, go and gd as undeclared and **not rs**. rs was the only binding still read as declaring floors it had deleted.

Reworded to describe rather than quote, with the reason recorded at the site so nobody restores the verbatim form. The audit now lists rs too — 6 → 8 undeclared dimensions, exit 0 — and rs's `make check` is exit 0 with zero `result: FAILED`.

This is the third appearance of one failure mode: a guard anchored on a **literal** rather than a name. py hit it, rs shipped it, and `#lzorderinganchors` — still queued — is the same thing in lazily-spec's `ORDERED_CHECKS`. Worth treating as a family-level hazard rather than three coincidences.

### Stated plainly

go's retired names now read as undeclared in the central audit, which is a **NOTE and not a failure**, and the same state py and gd are in. That is expected, and it means the audit's "declared floor" dimension is now mostly unused across the family — if it is never going to be declared again, it is dead surface, and that is worth deciding rather than letting drift. I have not filed it, because `#lzscenariofloormissing` in the queue is adjacent and may subsume it.

I also let one CI wait exceed its timeout and move to the background rather than shortening it; the result came back green for both commits, so nothing was reported before it was proven.

### Re: `#lzgoasyncflake` — root-caused, and `got 127, want 8` is what cracked it — opus-5 · 2026-09-12T06:01-04:00

> **Queue prompt:**
>
> do [#lzgoasyncflake]


Fixed, not waived. `a4faa3a`, CI green, pointer `c9c2d67a4`. And it **reproduces deterministically at `GOMAXPROCS=1`**, so it was never really a flake — it was a starvation-sensitive defect that the default scheduler usually hid.

### The number redirected the diagnosis

```
DIVERGENCE AsyncContext/churn_returns_to_baseline.json#3:observed_count — got 127, want 8
```

I had pointed at go's history of reading off the context lock, and a torn counter read gives 7 or 9. It gave 127. That rules out the lead I was most confident in, which is the useful thing a real number does.

### The cause was in the conformance model, not the library and not the fixture

`asyncModel.settle()` tested quiescence by scanning `ctx.effects` for a handle with `running` set. But `AsyncEffectHandle.DisposeAsync` deletes the handle from that map **while its body goroutine is still in flight** — it cancels the goctx, and `runningEffects` is only decremented later when the completion posts back through `onEffectDone`. So a **disposed-but-still-running effect is invisible to the scan** and correctly counted by the counter.

Step 2 of the fixture is 500 `dispose_then_create` cycles at `live_width: 8` — roughly 4000 dispose-while-running events — so its settle returned with bodies still pending. Those runs then landed *after* step 3's `runsBefore` watermark, and `observed := runLog()[runsBefore:]` counted them as step 3's own. Step 2 still passed its own `dependents_of` check because edge bookkeeping is synchronous in the loop and only the bodies lag. At `GOMAXPROCS=1` the settle loop's do/sleep cycle starves those bodies outright, which is why forcing the scheduler made it certain.

The part I find most instructive: **AsyncContext already maintains `runningEffects` and `pendingReruns` for exactly this question** — its own comment says the pair "make quiescence an O(1) test", and `resetDrainIfQuiescent` uses it. The conformance model re-derived a weaker predicate *beside* the correct one that was already there. That is a different failure from a missing guard, and harder to see, because the right answer was sitting adjacent the whole time.

On my three leads: the context-lock lead was the right **shape** but the read was properly locked — the defect was an incomplete predicate, not an unlocked one. The fixture is correct and **no assertion was relaxed**: 8 is exactly right for a publish to 8 live subscribers. Varying the scheduler was the instrument that worked, first try.

### A vacuous green in the same predicate

Worth calling out separately because it is this repo's recurring shape: `do()`'s return value was **discarded**, and the verdict was initialized to `idle = true`. So a sample whose closure never ran reported quiescence it had never observed. Same family as asserting fixtures ran from what the suite *loaded* rather than what it *did*. Now the return value is read, and a settle whose 30s deadline expires with work in flight reports a fault naming all three counts and fatals at both settle sites — it used to return silently.

### Both outcomes landed, not just the fix

I offered root-cause-and-fix or make-the-next-one-diagnosable. It did both. An `observed_count` divergence now names its window:

```
observed_count context: 127 run(s) in this step's window, want 8;
runs=[127 run(s), 8 distinct] live_0 x15, live_1 x16, ... live_7 x16
```

Eight distinct names at about sixteen runs each *is* the preceding churn, visibly — the distinction a bare count cannot make. Capped at 12 distinct names with the rest tallied.

And it is proven to bite: `TestAsyncSettleWaitsForDisposedRunningEffects` asserts as a **precondition** that the scan sees nothing while `runningEffects` is 1 — documenting the hole rather than assuming it — then that settle waits. Against the old predicate it fails. Restoring the old predicate reproduces 127-vs-8 at `GOMAXPROCS=1`; with the fix, zero divergences across `GOMAXPROCS` 1/2/8/32, `-cpu=1,2,8`, and `-race`.

### Two side checks it reported rather than skipped

- **Not** a `#lzrsbindpending`-style optional read: `observed_count` is dispatched from a fail-closed `expect` switch whose `default` fatals on an unknown key, and the comparison always runs. It can diverge; it cannot be silently skipped.
- Noted and deliberately not changed: `clearDrain()` clears only `lastDrainExhaustion`, not `drainIterations`. Harmless here — worst case ~8000 runs against a `DefaultDrainBudget` of 100,000 — and it was ruled out before the real cause surfaced.

### Stated plainly

This one cost about ten minutes of scheduler variation after seven useless clean runs, which is the lesson worth keeping: for an intermittent failure, changing the *conditions* beats repeating the *experiment*. I had that in the brief only because the earlier reruns had already failed; it should be the first instinct, not the third lead.

### Re: `#lzunreachableunprobed` — there was no branch to probe, and the type is why I thought there was — opus-5 · 2026-09-12T06:29-04:00

> **Queue prompt:**
>
> do [#lzunreachableunprobed]


Closed. `3f48c86`, CI green, pointer `f16a6baa8`.

### The premise was false

I filed this saying rs's `unreachable` ledger class "is enforced but unexercised, so its failure path is unprobed". It has no failure path of its own. Checked before touching anything:

- `CLASS_REASONS` mapped both classes to `True`, and **no code ever read a value** — no `CLASS_REASONS[...]`, no `.get`, no `.values()`.
- The class is used exactly three ways: a membership check (`klass not in CLASS_REASONS`), a per-class tally for the report, and display in messages.
- **No rung branches on which class an entry carries.** Suppression is the same code for `bind-pending` and `unreachable`.
- The one enforcement that exists — refusing an unknown class — was already probed during `#lzledgerceiling`, which reported unknown class, empty reason and duplicate each failing with their own message.

So an audit of the `unreachable` path was an audit of something that does not exist. That is three filings in a row of mine that did not survive being checked: this one, `#lzzigenvsecretleak` (the leak did not reproduce), and `#lzgotypedfloors` (the floors were not the only shrink detector).

### What actually misled me, and what I fixed instead

The **type**. A map whose every value is `True` reads as though the value carries semantics — that is why I assumed `unreachable` had its own enforced behaviour. Nothing in the code said otherwise.

It is now a set, `LEDGER_CLASSES`, with the reason recorded at the site: the class is descriptive, enforced only as a membership check, and it exists to make a distinction visible in the diff. I also wrote down *why* the spelling mattered, so the next reader does not repeat my inference.

### Why the vocabulary stays even at zero entries

Deleting it was the other option in the item, and it is the wrong one:

- **lazily-kt carries 25 real `unreachable` entries**, so the concept is live family-wide; rs dropping the vocabulary would make the two ledgers describe different things.
- Without the class, a genuinely unbindable site gets labelled `bind-pending` — which is exactly the laundering the size pin refuses, and the guidance at the pin already says *"Never raise it to park a `bind-pending` site."*

The interaction with `#lzledgerratchet` is worth naming: with `EXPECTED_LEDGERED_BLOCKS` exact at 0, rs cannot add any entry of either class without deliberately raising the pin in the same commit. So in rs the class distinction is currently unreachable **by construction**, not merely unused — which is a stronger statement than the item made and the right reason to leave the vocabulary alone rather than exercise it artificially.

### Verification

`make check` exit 0, zero `result: FAILED`, zero error/panic lines, and the gate still reports `0 ledgered of exactly 0`. I also evaluated the new set literal directly to confirm it holds exactly the two members — my first check used a regex over the block and caught the phrase `"not done yet"` out of a comment, which would have read as a third class.

### Stated plainly

This produced a small type fix and no new coverage, because none was possible. The honest summary of the cycle is that the item should not have been filed in the shape it was — "enforced but unexercised" was an inference from a data structure, not an observation of the code, and one `grep` would have settled it before it reached the queue.

### Re: `#lzdartboundfloor` — both floors deleted, and the second reason I gave was wrong — opus-5 · 2026-09-12T06:43-04:00

> **Queue prompt:**
>
> do [#lzdartboundfloor]


Done. `50ebb8a`, CI green on all three jobs, pointer `c585462a7`. `make check` exit 0, zero `result: FAILED`.

### `MIN_BOUND_BLOCKS` was implied, and it was measured rather than argued

The agent instrumented the guard's OK line instead of reasoning about the arithmetic:

```
IDENTITY HOLDS: bound_count 700 == declared_sites 725 - excuses 25
```

and held in the other reachable green states too — `699 == 725 - 26`, and `700 == 725 - 25` with a fixture dropped. Not a coincidence of today's corpus: reaching that line means the `problems` loop found every declared site either bound or excused, and the stale-excuse direction means none is both, so the sets partition and both sides are already pinned.

The ordering detail it found earlier is the sharper form of the argument: `if problems: die` sat **ahead** of the floor, so the floor was only ever *reached* in the state where every unbound block was already excused — which is precisely the state `EXPECTED_LEDGERED_BLOCKS` owns. All three ways to move `bound_count` now fire upstream of it: a detach with no excuse hits the `problems` loop by name, a detach plus matching excuse hits the size pin (`GREW to 26 site(s); the pin is 25`), and a corpus shrink hits the magnitude rung from either side.

**And the slack was reproduced live.** Detach-and-excuse takes bound to 699; with the size pin satisfied at 26, the floor at 697 is reached and prints `unbound-block guard OK: 699/725`, **exit 0**. It cannot see a one-block shrink, let alone a three-block one.

### The correction: `MIN_BLOCK_FIXTURES` was **not** implied by the site-set equality

I asserted it was, on the grounds that sites are keyed by fixture. That is wrong, and the reason is specific: **8 of the 144 opened fixtures carry zero assertion blocks** — all of `statechart/`. I verified this independently rather than taking the report: walking all five tracked block names across those eight files gives **exactly zero** blocks in every one.

So the site set spans only **136** fixtures, and the fixture set is not recoverable from it. Dropping `statechart/flat_cycle.json` from the manifest leaves the post-deletion guard green at `700/725 ... across 143 opened fixtures`. The floor did catch that, with zero slack — 144 against 144.

What covers it is one rung up, in `check-conformance-coverage.sh`: the opened-set-equals-corpus-minus-`KNOWN_UNCOVERED` identity, which on the same doctored manifest fails with `canonical fixture 'statechart/flat_cycle.json' was NOT opened by the suite`. Both guards run in `make check`, so the case stays covered — and it moved from a **count** to a **set identity that names the fixture**, which is the better failure. The deletion is still right; my justification for half of it was not.

### The follow-up it escalated instead of taking

It noticed that `derive_expected` already returns `expected_fixtures` (144) and `main` already computes `examined` (144), so `examined == expected_fixtures` would close the statechart hole *inside* this script — derived, fails-when-stale, no new state. It left the call to me rather than landing it, on the grounds that it would be a second definition of an identity the coverage guard already asserts.

**I'm declining it, and that reasoning is why.** Two definitions of one identity is the duplication this whole series has been removing, and the existing failure is strictly better — it names the fixture rather than reporting a magnitude. Both guards run in `make check` and in CI. If they ever diverge, the fix is to make one of them authoritative, not to add a third.

### Filed

`#lzdartcoveragefloors` — dart's *other* guard still carries `MIN_FIXTURES=144` and `MIN_SCENARIOS=153` at `check-conformance-coverage.sh:371-372`, the same implied shape in a different file, subsumed by the identity assertion at line 155 of that same file. The item carries both traps this session has already paid for: keep the zero-guards py and go kept, and describe the retired constants in prose rather than spelling their assignment form, because that is what left rs reading floors of 150 and 166 out of its own comments.

### Zero-guards and hygiene

All three zero-guards still fire after the deletion, probed with emptied populations — absent manifest, manifest resolving to nothing on disk, and a manifest of only the eight block-free statechart fixtures (`the 8 opened fixtures carried ZERO assertion blocks`). The expectation side too: an emptied canonical corpus reports `found 0 fixture(s), 0 site(s) and 0 distinct block(s)` rather than agreeing with an inventory of zero.

`lazily-spec/conformance` was never written — the probes needed a scratch sibling layout because `canonical_corpus_dir()` ignores `LAZILY_SPEC_CONFORMANCE_DIR` by design, verified byte-identical before and after by md5. The three surviving mentions of the retired constants are prose with no live assignment, and spelled bare rather than in assignment form.

### Re: `#lzdartcoveragefloors` — done, and the reasoning I gave three bindings was false — opus-5 · 2026-09-12T07:02-04:00

> **Queue prompt:**
>
> do [#lzdartcoveragefloors]


`dart d1ba1e1` and `rs 18c71bd`, both CI green, pointer `b9cbaeba1`. dart's `MIN_FIXTURES=144` and `MIN_SCENARIOS=153` are gone, replaced by corpus-derived equalities with duplicate checks and four zero-guards, matching go. Both derivations land on the numbers that were typed, computed, with no constant left in the file.

### The correction, which is the real content of this cycle

I told go and then dart that the equality earns its place because *"a duplicated excuse entry passes both directions while making the count disagree."* I took that from rs's own comment. **It is false, and it is false in rs too.**

`uniq_known` is computed as `sort -u | wc -l`. Both sides of the equality **dedupe**, so a repeated entry moves neither. dart measured it rather than believing me — with the duplicate check neutralised and the dedupe left in place:

```
conformance coverage OK: 144/156 canonical fixtures OPENED (12 listed as known-uncovered;
144 DERIVED from the corpus listing minus that ledger and asserted EQUAL...)   EXIT=0
```

Same on the scenario side: one excuse and two copies of it derive identical arithmetic. The equality only fires on a duplicate if the count is taken naively as `${#KNOWN_UNCOVERED[@]}` — and then its message is the misleading one about trees not matching.

**So the `uniq -d` duplicate check is the whole of what closes that gap.** I verified the mechanism across all three bindings: rs at `:571`/`:580`, go at `:386`/`:396`, dart at `:411`/`:421` — every one dedupes *and* carries an explicit duplicate check. The check works; the equality never did.

Nothing shipped broken, because go and dart both landed the duplicate check on my instruction — I asked for the right artifact for the wrong reason. But the reasoning was load-bearing for three bindings, so I corrected it at the source rather than filing it: rs's comment now says what the equality actually buys (a tripwire if a loop is ever weakened, plus the magnitude in the OK line), states plainly that it does **not** catch a duplicate, and records that the claim propagated through me until dart sent it back.

### The agent held its own position, which is the other thing worth noting

I asked whether it still believed these equalities are implied, having argued the opposite about `MIN_BOUND_BLOCKS` an hour earlier. It said yes, and gave the theorem: once `missing` is 0, the forward loop makes every corpus fixture opened-or-excused and the stale loop makes every excuse name an unopened corpus fixture, so `covered == total - uniq_known` identically, and the two walks then span the same fixture set so the scenario total follows.

Its probe table is that theorem showing up as a procedure — **every single probe had to neutralise a loop to reach the equality at all.** It kept the equalities for parity with go and rs and flagged the decision in the commit message rather than burying it, so the family can strip them everywhere if it wants. That is the right way to hold a position I had pushed against.

### Verification

`make check` exit 0 with zero `result: FAILED` in both repos. Four zero-guards fire in dart — empty corpus, every fixture excused, every `scenarios` array stripped, every scenario excused. `check-corpus-floors.mjs` exits 0 at `156 fixtures pinned … 10 dimension(s) undeclared`, 8 → 10 as predicted, with dart now in the NOTE beside rs, py, go and gd. Neither retired name appears under dart's `scripts/` in any form — it described them without naming them, so all four spellings the audit matches are absent. And it independently confirmed rs is out of the trap I fixed earlier: the audit no longer reads floors of 150 and 166 from it.

### Stated plainly

That is the second consecutive cycle where the agent corrected my premise, and the fourth in a row overall. The pattern in all four is the same: I stated an inference as an observation. Here the inference was inherited from a comment in the reference implementation, which is the most seductive form — it looked like established doctrine rather than something to check, and it survived three bindings because each time I cited it instead of testing it. The `sort -u` was on the line below the claim the whole time.

### Re: `#lzdartarrayblocks` — 12 element sites bound, and the gap is exactly 12 corpus-wide — opus-5 · 2026-09-12T07:16-04:00

> **Queue prompt:**
>
> do [#lzdartarrayblocks]


Done. `510e1c1`, CI green on all three jobs, pointer `c7cf0e252`. `make check` exit 0, zero `result: FAILED`.

Sites **725 → 737**, exactly +12 and confirmed rather than assumed. Digests **616 → 628**, so all 12 elements are content-distinct. Bound **700 → 712**. `EXPECTED_LEDGERED_BLOCKS` stayed **25** and `KNOWN_UNBOUND_BLOCKS` is untouched — these became bound, not ledgered, which is what I asked to see.

### What the groundwork changed before dispatch

I established three things myself rather than relaying the item, and each one altered the work:

- **The 12 are real assertions, and dart's runner already asserted them** — frame count, per-element target, every key of each expected frame. So this was the bind-pending shape: read and asserted, missing only the routing. Falsifying a value was already caught; a runner that silently stopped asserting them was not.
- **lazily-spec already had the vocabulary and the pin.** `check-corpus-floors.mjs:177` emits a `${key}[]` bucket per plain-object element, `corpus-counts.json` pins that fixture at `{"assertions": 1, "expect[]": 12}`, and the audit defines the bindings' current dimension literally as `!bucket.endsWith("[]")`. So widening dart aligned it with a dimension the spec already tracked.
- **The gap is exactly 12 sites in exactly 1 fixture, corpus-wide** — 783 objects-only + 12 = the audit's 795. Nothing else carries an array-valued tracked key.

### The part I would not have specified well

Over-widening is the real hazard here and the corpus exercises only one shape, so the agent probed the walk on **synthetic** shapes: object value unchanged, array of objects emitting both, array of scalars emitting none, **mixed array keeping true indexes** (`expect[0]`, `expect[2]`), nested array `[[{...}]]` emitting none because the rule is one level, and an untracked key still not a block. That table is the guardrail, not the corpus.

It also widened `isAssertionBlockPath` and the guard's `walk` **in step and pinned them against each other**: a recorded site the walk does not enumerate fails as a corrupted evidence channel, and an enumerated site nobody records fails as unbound. Either being wider than the other makes a green run impossible.

And disambiguation was proven rather than assumed — detaching two elements of the same 3-element step names `steps[2].expect[0]` and `steps[2].expect[2]` **separately**, where a per-array label would have collapsed them. Detaching the middle element alone names `steps[2].expect[1]` exactly. Throughout every detach probe, `dart test` stayed green at 733 tests: the suite itself never misses them, which is precisely why rung 0 has to.

### The higher rung, fixed and honestly reported

The per-key loop visited only keys the **fixture** named, so a field the room emitted that the fixture omitted was compared by nothing — the same weakness rs's stdlib had. Since `frame` is object-valued, the tracker now requires `assertKeySet(exp, 'frame', actualWire.keys)` in both directions, ahead of any value comparison, and `to` moved from a bare read to `assertKey`.

It then said plainly that **the key sets already agreed for all 12 frames**. The hole is closed; it did not expose a live divergence. That is the right way to report a guard that found nothing.

### Filed, and smaller than I expected

`#lzarrayelementsites` for the remaining bindings — and the agent found this reduces the scope: **`lazily-js` is already on the wider dimension** at 747/638, which I verified from the audit's per-binding rows. So it is **eight** bindings, not nine: rs, py, kt, go, cpp, zig, cs, gd. Each already has its target numbers computed and waiting in `--report-blocks` (rs 783/673 against its current 771/661), and the item carries dart's three load-bearing techniques plus the expectation of one higher-rung failure each.

### Stated plainly

dart now sits on lazily-spec's "all five names, + array elements" row while eight bindings sit on "objects only", so cross-binding site counts are temporarily incomparable between those two groups. That is a real cost of doing one binding first, and it is the reason the follow-up should not sit long. The audit itself is unaffected — exit 0, still `795 assertion-block site(s) across 148 fixture(s) pinned exactly`.

### Re: `#lzarrayelementsites` — seven bindings, +12/+12 each, and four silent holes — opus-5 · 2026-09-12T08:01-04:00

> **Queue prompt:**
>
> do [#lzarrayelementsites]


Done. All seven CI green, pointers in `794cc32a2`: `rs 880caa5 · py 1242dc1 · kt d83d6ba · go f912a63 · cpp 23b46d4 · zig ddfa8e9 · cs c7531ce`. The audit exits 0 and `lazily-spec/conformance` shows **zero** dirty lines — no agent touched the shared corpus.

Every binding moved by exactly **+12 sites and +12 digests**. The clause "a runner binds elements, not the array" had been pointing at a site nobody emitted since it was written.

### What the parallel dispatch bought, and what it cost me

I parallelised seven agents against one shared rule file because each had an **externally computed oracle** — the audit's per-binding row — so a wrong walk could not land green. That held. It also exposed that **two of my oracles were wrong**, and in both cases the agent refused to reconcile:

- **go reads only three block names** (`assertions`, `expect`, `expected`), so it sits a row lower on the name axis. I gave 731/622; it was at 725/616 and verified that three ways before reporting it.
- **cs had never landed the five-name widening either** — its `SpecCorpus.AssertionBlockNames` carried three. I gave 749/640; it was at 743/634, and it did *both* widenings to reach 761/652, measuring at each step.

My check for this was itself wrong: I grepped eight lines after each name-constant occurrence across the whole repo, which unioned the guard's python twin with the live constant and reported five names where the binding read three. A repo-wide union is not a live value, and that is the same class of error as the four premise mistakes earlier tonight.

kt's absolute numbers also differ — 737 → 749 rather than 731 → 743 — because the spec narrows kt's opened set by `REQUIRED_AREAS` while `ProtobufGraphBoundaryConformanceTest` opens `protobuf/graph_boundary_traces.json` anyway. The audit says so itself and **declines to derive a floor for kt for exactly that reason**, which is `#lzktareapartition` in the queue. The +12/+12 delta is identical either way.

### Four silent holes, all fixed rather than excused

- **cpp's is the worst thing found tonight.** The element loop indexed delivered queues **by the fixture's own `to`**, so it could only see frames the fixture asked about. Deleting `steps[2].expect[2]` left the room genuinely delivering a peer-joined frame to conn `b` that nothing consumed and nothing reported — **green at 11 frames**. A fixture could under-specify and the runner would never know. Now a produced-versus-declared routing multiset, both directions, order-independent. Two more alongside it: a produced field the fixture omits, and a fixture key no call site handles. Plus an ill-typed `std::get<ServerError>` that **threw rather than failed** when the room produced a different alternative.
- **kt proved its own hole live.** A third key on an element was compared by nothing; planting `deliver_after_ms` on `steps[2].expect[1]` passes green on HEAD and now fails by name. Its frame comparison had been model-against-model, so a field the corpus declares that the model does not carry never reached a comparison.
- **rs caught its own first draft being vacuous.** It bound `frame` by whole-value equality against its own decode — and `decode(fixture.frame)` re-serialised *is* `fixture.frame`, a self-comparison. Its probe respelling an expected `sdp` stayed **green**. Fixed by descending and comparing against values derived from the decoded input and the connection registry, with the welcome roster derived from who was in the session *before* the join. Its mutation sweep then went **8 of 9 green before to 0 of 9 after**, with an unmutated negative control.
- **rs also deleted a ledger excuse that was simply false** — it claimed rs ships the signaling codec but not the server session producing the route, when the route is derivable from the `conn → peer` registry the runner already builds.

zig settled the label question with a decision and a probe rather than a preference: element labels are always `[<index>]`, with the `name`-preferring convention scoped to arrays reached by descent, because the twin derives site counts from the corpus with no access to a runner's naming convention.

### Filed

Five items, all from measurement: `#lzgothreenames` (go's name axis, with the `NodeValue` found-bool defect that passes vacuously), `#lzcsreverserung` (cs has no reverse rung; **55** bound-but-undeclared digests today, which is why its 12 could sit bound and undeclared), `#lzpyelementdescend` (py does not descend into an emitted element, diverging from the spec's deliberate rule), `#lzsemtreesubset` (the subset-assertion design question go and cs hit independently), and `#lzarrayelementdocs` (rs's and kt's superseded prose, both correctly left to their owning panes).

### Stated plainly

py's divergence is the one I would watch. It is unobservable today — rs and zig each confirmed independently that no corpus fixture nests a tracked key inside a tracked array — but the spec's own comment exists precisely so the first fixture to do so is pinned by the change that adds it. py would be fixing `instrument` under pressure rather than deliberately, which is the situation that comment was written to prevent.

### Re: `#lzgothreenames` — six blocks bound, and three vacuous passes came with them — opus-5 · 2026-09-12T08:16-04:00

> **Queue prompt:**
>
> do [#lzgothreenames]


Done. `6a1a6b9`, CI green, pointer `0987d4286`. **737 → 743 sites, 628 → 634 digests**, exactly the audit's five-names row, `+6/+6` — the numbers the agent measured last cycle held with no reconciliation. All six blocks bound, `EXPECTED_LEDGERED_BLOCKS` still 0, `unboundBlockExcuses` still empty, 743/743 bound.

The detach proof is the one that matters for this item: detaching the three `expect_initial` binds names `.scenarios[0..2].expect_initial` separately, detaching both kinds names all six — and in every case `TestCollectionsSemTreeIncremental` **stayed green**. The comparisons were running the whole time; they were simply invisible. Binding them makes the rungs above apply, demonstrated by skipping one key's comparison and getting *"key `a` is read by the runner but never reaches a comparison against the fixture's own value"*, which before the widening was reported by nothing.

### Three defects, all green on HEAD, all one shape

The shape is **an API returning a zero value for something absent, read as a measurement**:

1. **The discarded `found` bool** I filed this for. Planting `"ghost_node": 0` in scenario 0's `expect_initial` is `ok` on HEAD. Now named. The fix detail is better than what I asked for: presence goes through `NodeHandle`, **not** `NodeValue`, because `NodeValue` pulls the slot and a pull is exactly what `sibling_a_cached` measures — checking presence the obvious way would have perturbed the thing being asserted.
2. **`IsCached` on an absent node** — same class, different API. It also returns false for a node that does not exist, so `sibling_a_cached: false` was satisfied by the sibling's *non-existence*. Proven by renaming scenario 1's node `a` to `z` and asserting `sibling_a_cached: false`: green on HEAD.
3. **`got != (want == true)`**, and this is the worst of the three. `want` is an `any`, so `want == true` is false for **every** non-boolean — `0`, `null`, `1`, an object, and the string `"true"`. Planting `"downstream_consumer_reran": "true"` where the run observes `false` is **green on HEAD**, while the fixture reads as asserting the consumer *did* re-run. Not a missed assertion — a **silently inverted** one that passes.

It also explained why cs, which bound the same six blocks, hit neither 2 nor 3: `tree.Derived(key)` throws on a missing node in C# and its comparison goes through a typed decode. Both are go-specific holes in `NodeValue`/`IsCached`'s `(zero, false)` contract. That is the kind of cross-binding reasoning that makes the finding actionable rather than anecdotal.

### Filed, because it is almost certainly not go-only

`#lzflagcoercion` — audit every boolean-flag comparison that reads a fixture value through a dynamic or variant type, in the other eight bindings. Any binding whose fixture value arrives as `any`/`dynamic`/`object`/`JsonValue`/`std::variant` and is compared by equality against a native `true` has the same hole, and cs shows the shape of the fix. The item also carries the broader hunt: grep for accessors returning a `(value, found)` pair or an `Optional` whose second half is dropped at a conformance call site.

### Left open as directed

The run-to-fixture direction, with a comment at the site naming `#lzsemtreesubset` and stating why answering it there would decide a ten-binding corpus contract unilaterally and in the wrong repo. No fixture edited, no ledger entry added.

### Stated plainly

Both halves widened in step by construction — one `walkAssertionBlocks`, both sides call it — and it probed that anyway by narrowing each call site in turn: the derivation narrowed gives `743 … expected exactly 737 — 6 MORE`, the inventory narrowed gives `737 … expected exactly 743 — 6 FEWER`. Neither one-sided state lands green.

Worth noting what this cycle says about the earlier ones: go was the binding I got wrong in the `#lzarrayelementsites` oracle, and the reason it was wrong — three names instead of five — turned out to be hiding three vacuous passes. The wrong oracle was a symptom of a real defect, which is the best possible outcome for having been wrong.

### Re: `#lzflagcoercion` — eight bindings audited, eight at risk — opus-5 · 2026-09-12T09:00-04:00

> **Queue prompt:**
>
> do [#lzflagcoercion]


Done. All eight CI green, pointers in `33ed9e07e`, and `lazily-spec/conformance` shows **zero** dirty lines across all eight audits: `py 19aeb4a · js 5a113e1 · kt 4d32089 · dart 109fb28 · rs 92fd0d9 · cpp 97790fd · zig c656ded · gd cd8c3b7`.

go's `got != (want == true)` turned out to be one spelling of a class, and **every binding was at risk somewhere**. The coercion half varies by language; the absence half is universal.

| binding | coercion half | absence half |
|---|---|---|
| py | 5 sites (`if expected:`, `bool()`, `set()`, `int()`) | 2 |
| js | 8 sites (`Boolean()`, `if (want)`, `=== true`) | 1 declined, unprovable without a corpus change |
| kt | 9 sites (`?: false`, `booleanOrNull?.let`, string-tolerant `boolean()`) | included above |
| dart | 8 sites — **go's defect verbatim** | 4 |
| rs | 6 sites (`unwrap_or(false)`, folded `None`s) | 8 |
| cpp | 17 sites — an `as_bool()` returning a default-constructed member | 2 |
| zig | **none** — all four paths fail-closed, proven by plant | 6 |
| gd | 4 sites (numbers coerce silently) | 5 |

### Three results above the fixes

**cpp's corpus already carried a probe for this class, and strictness in the wrong place broke it.** `statechart`'s `internal`/`parallel` were gated by a `chart_require` that *throws*, because `malformed_rejected.json` pins that the builder **rejects** a non-boolean. Routing those through the new abort broke two corpus cases; the suite caught it and the agent reverted rather than weakening the fixture.

**rs produced the sharpest single measurement:** one plant, `invalidates.membership: "true"`, is a named failure in `collections_conformance.rs` and was **green** in `collections_family_conformance.rs` — the family runner being the coerced copy of a runner that already got it right. kt and js found the same sibling-runner masking independently, and dart reasoned it correctly in the opposite direction, leaving stdlib's `predicate == true` alone because the library driver casts the same key `as bool` in the same run.

**gd found the worst failure mode.** In Godot 4 a mistyped flag mostly *raises* — and the abort takes out the enclosing `for key in expect` loop, so every **sibling** key in the block goes unasserted while the runner's own `failures` list stays **empty**. The string-plant run reddened only because gdUnit4's error monitor happened to be armed: `4 errors | 0 failures`, the conformance channel silent about all four.

### My instructions were wrong twice, and both were caught by measurement

- **gd's coercion premises were Python's.** I wrote that `1 == true` is true and `bool("false")` is True. In Godot 4, `float == bool` **raises** and there is no `bool(String)` constructor — so go's inversion is unreachable there. What is real is that `var x: bool = 1.0` coerces silently.
- **`grep -c "result: FAILED"` is always 0 in gd**, because gdUnit4 never emits that string. I have been asking every agent all night to use it; in that binding it is vacuous. The agent substituted `Statistics: … N failures` plus an explicit `$?`.

Two other subtleties worth keeping: rs's `check!(.., got, want.as_i64())` folded **two different `None`s** — "the read failed" and "this expectation is not a number" — so they matched each other and `value: "9"` on a failing read was green. And zig's `crdt_plane` read union fields bare, which is not merely a poor diagnostic but **unchecked UB under ReleaseFast**.

### Filed

`#lzsiblingrunnermasking` — the structural problem three bindings found independently, with cpp's shape as the fix: it **deleted** `as_bool()` so the weak spelling is a compile error rather than a convention, and added a hygiene rung. Per-site fixes do not survive a runner being split or skipped, and a skipped Gradle `:test` reads as a passing one under RTK.

`#lzplantprobemethod` — the probe method and its three failure modes, all hit tonight: a **vacuous plant** (rs wrote a gate onto a scenario carrying neither the gate nor the gated thing, and the green briefly looked like a 15th defect), a **plant that reddens before** (cpp had 3 of 16 and refused to present them as probes), and a **plant masked by a sibling runner**. Plus the two environment traps above.

### Stated plainly

Every agent enumerated rather than only fixing, and several reported sites they deliberately left with the reason — rs proved `matches!(v, Value::Bool(true))` appears nowhere, py demonstrated `observed_by`'s char-iteration hazard is unreachable by planting both `"watch"` and `"w"`, kt found its own `boolean()` doc justified string tolerance with a claim about the corpus that a structural walk of all 156 fixtures disproved. js declined a fix it could not prove — `isCached` on an absent node, go's defect on the same key name — because no corpus edit isolates it, and flagged it for whoever owns the corpus instead of shipping an unproven change.

`sibling_a_cached` has now appeared as a defect or a hazard in **five** bindings. That is not five coincidences; it is a corpus discriminability problem, and it belongs to `#lzsemtreesubset`.

### Re: `#lzsiblingrunnermasking` — five enumerations, and the evidence channel turns out to be conditional — opus-5 · 2026-09-12T10:28-04:00

> **Queue prompt:**
>
> do [#lzsiblingrunnermasking]


Done. All five CI green, pointers in `92f42039e`, corpus never written: `kt 58eae6e · dart 3e5cf38 · js 8e04a0c · cpp a14d14d · rs 4e1fbe0`.

Every binding enumerated its runner pairs from **runtime evidence** rather than source — each test file run alone against its own manifest — which is a better method than I specified and all four who did it arrived at it independently. cpp corrected my framing: **38 fixture-reading binaries**, not the ~10 files I said.

### The masks were real, and three were live

- **cpp**: planting a mis-nested `invalidates` one level up left `QueueConformance` **and** `TopicConformance` both passing — only the family binary was red. Now refused at the single door for all 143 fixtures, derived from the corpus (all 337 `invalidates` sit inside an assertion block).
- **dart**: `queue_conformance` compared `returns` inside the pop arm only, so `bounded_backpressure` step 2 and `closure_lifecycle` step 6 were read by **nothing** in that runner.
- **rs**: `initial.closed` was read by one runner alone while both family flavors built an open queue **whatever the fixture said**, though all five fixtures declare the key. Also `steps[].returns` matched by *prefix*, so `returns: ""` passed against any return.
- **kt** found a **mutual** mask: WorkQueue caught a dropped reader kind and was blind to an added one; Family the reverse. Each runner covered exactly the half the other missed.

### cpp closed its own allowlist, and the cause was worse than it had described

The allowlisted file carried the **original** copy of the fixture JSON type — `test_json.hpp` had been extracted from it and the original stayed behind. Two `Json` types, two default-constructed `boolean` members, one watched. Removing it also retired `assertion_json`, a hand-maintained clone that had to stay content-identical for rung-0 fingerprints and **silently did not for every number-carrying block**. The proof it is identical now is the invariant itself: **bind magnitudes byte-identical across the removal.**

It also found the ideal end state already present in one pair — `Materialization`/`MaterializationFamily` both call a shared `replay_materialization<Model>()`, so divergence is **not expressible** rather than merely absent. That is the target shape for the rest.

### rs's falsification table is the argument for a rung beside a lint

`clippy.toml` bans `Value::as_bool` and `make check` runs clippy with `-D warnings`. Clippy alone **passes four of the eight probes**: an `#[allow]` at the call site, an emptied `clippy.toml`, `as_array().map(..).unwrap_or_default()`, and an `#[allow]` in `src/bin/`. A lint a developer can locally silence is not a guard.

rs also explained what shrank its search, and it transfers: `Expect` fails at drop on any key of a guarded block the runner did not consume, and every runner guards the block it replays — so key-level asymmetry **inside** a block is impossible there. The residual surface is exactly values read off the raw `Value` outside any tracker, plus comparison strength. That is why all three of its masks were in the first category.

### Three things filed, and the first outranks most of what is left in the queue

**`#lzstalemanifest`** — kt's answer to the question I asked it. The guards read `build/conformance-fixtures-loaded.txt` and both ledgers with **no freshness check**, so a cached `:test` leaves the previous run's manifest and every "these bytes were really read" rung accepts it as this run's evidence. Its `make check` exited 0 with `:test UP-TO-DATE` — nothing ran. The entire rung-0 apparatus this session built rests on that file, and RTK makes it worse by stripping the Gradle task lines that would show it.

**`#lzfailedgrepvacuous`** — `grep -c "result: FAILED"` is vacuous in **four** bindings, each proved rather than suspected, and I asked every agent all night to use it. It masked nothing, because exit codes were captured separately, but it contributed nothing either.

**`#lzastscanundercount`** — rs's `syn` walk saw **39 of 86** sites and reported the other 47 clean, because `syn` does not descend into macro token streams and most of its comparisons live inside `assert_eq!`. A rung that silently sees 45% of its target produces a green that reads as coverage.

### Stated plainly

Every agent reported an honest negative. rs had one probe redden *before* and labelled it a latent weak spelling rather than a demonstrated mask; dart did the same for a handle-identity case no fixture can isolate; js declined to claim two sites that redden anyway. rs also flagged proactively that it touched six sites from `92fd0d9` and said why — leaving them would have needed per-site clippy allows, which is the hole the rung exists to close.

js contributed the one structural fact I had not seen: ledger records are keyed `fixture\tblock\tkey`, so **an obligation any runner discharges is discharged for all**. Several apparent asymmetries are free rides on that global ledger rather than differences in strength — which means per-runner obligation accounting does not currently exist, and the enumeration is the only way to see it.

### Re: `#lzstalemanifest` — nine of ten were printing verdicts for runs that had not happened — opus-5 · 2026-09-12T11:17-04:00

> **Queue prompt:**
>
> do [#lzstalemanifest]


Done in all ten. Every commit CI-green, every magnitude unchanged, corpus never written, pointers in `2fefabc36`: `rs ccf96bd · py 6a600f9 · js 83e5ff0 · kt 9cbe103 · go 5dc05c4 · cpp 9b5af3d · zig 371ab5e · cs e9349f7 · dart 4ba46db · gd 9adce76`.

**Nine of ten had a live path on which a guard printed its full green verdict over evidence from a run that had not happened.** The tenth — rs — was structurally immune on `make check` and exposed on the guard invoked alone.

### The worst of it was not a cache

- **cpp's `make -j16 check` published a verdict before the tests started.** The guard ran at `10:38:17.69`; ctest was invoked at `10:38:26.04`. It reported `722 sites / 619 digests / 697 bound`, `143 fixtures`, `151/151 scenarios` — **8.3 seconds early, at exit 0**. Its `conformance-coverage` target had **no graph edge to `test`, only a comment claiming one**.
- **go reproduced the shape end to end**: guard green, then a cached run writing zero lines, then *the same green verdict* for a run in which the test binary never executed.
- **gd found accretion, not staleness**: a single hand-run scene put 250 records on top of a previous run's 317, and the guard reported green off **the union of two runs**.
- **py's guard printed `these bytes were really read`** over a 145-line manifest with no pytest invoked at all.

### The design converged on one point I had not specified

Stamp from the **producer**, never at truncation. zig put it best: a Makefile stamp *"would date evidence a skipped run never produced and turn the empty-manifest failure into a pass."* dart reached it independently — the id then comes from a process that actually replayed the corpus. gd stamps at truncation and is rescued only by the completion marker it also added, which is worth knowing as a design divergence rather than an equivalent choice.

### Four determinations that changed what I believed

- **zig disproved that `has_side_effects` was load-bearing.** Every `zig build` mints a random `--seed` that lands in each Run step's cache manifest, so a test Run step cannot be a cache hit. It proved this by *removing* the flag: 12 cached compiles, **zero** cached run steps. The real hole was the `: >` truncation paired with `[ ! -s ]` — one recipe line.
- **kt resolved the Gradle wrinkle from its own prior measurement**: a Test task's `environment` is untracked under `#lzktgradleinputfingerprint`, so the id feeds the JVM without touching the fingerprint. It deliberately refused to declare it an input, because that *would mask the bug by disabling the cache and leave the guard never once exercised*.
- **kt also named the distinction that makes a per-job id correct**: it separates "cached within this invocation" from "left over from an earlier one". Its CI log shows `:test` executing in one step and `UP-TO-DATE` in the next, with the guard green.
- **cs showed why its missing reverse rung cannot substitute.** A unioned manifest supplies a `bound` **superset** that rung 0 absorbs — a site reads as bound if *either* run bound it — and with **55 undeclared digests already tolerated**, that detector has no calibration headroom left. Existing slack destroyed the sensitivity of the thing that would have caught this.

### Two refused to apply the protocol where it would be wrong

rs left `benchmark-check` alone, arguing that its claim is "measured against **this checkout**", which its source fingerprint already discharges — and that a run id there **would have reddened that job**, since CI runs the evidence and check steps as separate make invocations. It stayed green in CI, confirming the call. cpp left the two wasm tier manifests unstamped with the reason in the Makefile: a separate channel whose CI job spans three make invocations, needing a job-level id of its own.

### Filed

`#lzstampsatisfiesnonempty` is the one to do next — js and cpp each independently found that **stamping makes every evidence file non-empty**, so existing `test -s` checks can no longer see a detached recorder. Two of ten caught it; the other eight may have weakened an emptiness check without noticing, and zig is most at risk since its entire prevention was truncation paired with `[ ! -s ]`.

Also `#lzstampprefixdrift` (go already machine-enforces the producer/guard prefix agreement that kt flagged as unenforced — adopt its test in the other nine), `#lzrscorpusrootvacuous` (a guard reporting `0 references across 0 files, all baselined` against a header describing 45 baselined sites), and `#lzmakepfootgun` — `make -p` builds the default goal, so rs's `make -p | grep -m1` started `make check`, ran the reset, and **SIGPIPE'd, silently truncating `build/` mid-analysis**. The analysis destroyed the evidence it was analysing, and the symptom looks exactly like the bug under investigation.

### Stated plainly

`grep -c "result: FAILED"` is valid in rs, which does emit it — so the vacuity filed under `#lzfailedgrepvacuous` is per-binding rather than universal, and that item now has calibration data from all ten. And RTK hid the cache indicator in **two** bindings: it stripped kt's Gradle task lines and filtered go's `go test` output into a summary that omitted `(cached)` entirely. The tool I rely on for output concealed precisely the signal this bug travels on, in both places it travels.
<!-- agent:boundary:ce9fbb66:lazily -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-commit-push use subagents for fan-out tasks" priority go -->
- 🚧 do [#lzstampprefixdrift]
- do [#lzstampsatisfiesnonempty]
- do [#lzrscorpusrootvacuous]
- do [#lzmakepfootgun]
- ~~do [#lzstalemanifest]~~
- do [#lzfailedgrepvacuous]
- do [#lzastscanundercount]
- ~~do [#lzsiblingrunnermasking]~~
- do [#lzplantprobemethod]
- ~~do [#lzflagcoercion]~~
- ~~do [#lzgothreenames]~~
- do [#lzcsreverserung]
- do [#lzpyelementdescend]
- do [#lzsemtreesubset]
- do [#lzarrayelementdocs]
- ~~do [#lzarrayelementsites]~~
- ~~do [#lzdartcoveragefloors]~~
- ~~do [#lzambienttokenresolve]~~
- ~~do [#lzzigemptyparentdump]~~
- ~~do [#lzpinparsestrict]~~
- ~~do [#lzledgerratchet]~~
- ~~do [#lzzigenvsecretleak]~~
- ~~do [#lzgotypedfloors]~~
- ~~do [#lzgoasyncflake]~~
- ~~do [#lzledgerceiling]~~
- ~~do [#lzunreachableunprobed]~~
- ~~do [#lzrsbindpending]~~
- ~~do [#lzdartboundfloor]~~
- ~~do [#lzdartarrayblocks]~~
- do [#lzorderinganchors]
- do [#lzktareapartition]
- do [#lzscenariofloormissing]
- do [#lzcireachsync]
- do [#lzfollowupqueueorder]
- do [#lzktbindpending]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] 🚧 [#lzstampprefixdrift] [lazily-rs, lazily-py, lazily-js, lazily-kt, lazily-cpp, lazily-zig, lazily-cs, lazily-dart, lazily-gd] The run-id stamp prefix now exists in TWO SPELLINGS in every binding — the producer writes `# lazily-run-id <value>` from test-side code, and the guard recognises it from a separate constant. lazily-kt flagged this explicitly as a caveat: a drift between them fails closed (the guard refuses), so it is safe by construction, but it is NOT machine-enforced and the failure would look like a stale-evidence bug rather than a prefix typo, which is a bad diagnostic for a confusing failure. LAZILY-GO ALREADY SOLVED IT and is the reference: `TestGuardAndRecorderAgreeOnTheStampPrefix` couples the Go constant to the bash `RUN_ID_PREFIX`, and it is one of four mutations that redden its probe suite. Adopt that test in the other nine. The general principle is the one this whole series keeps rediscovering: two definitions of one string, held together by a comment, is the shape that drifts — lazily-cpp found the same thing structurally this cycle when its `assertion_json` clone had silently diverged for every number-carrying block, and lazily-rs found it in scenario-id resolution that agreed with the shared rule only by coincidence of input. A test that fails when the two disagree costs a few lines.
- [ ] [#lzstampsatisfiesnonempty] [audit all ten, lazily-js and lazily-cpp already fixed] THE RUN-ID PROTOCOL INTRODUCES A HAZARD IT MUST ALSO CLOSE, and exactly two bindings caught it independently. Stamping an evidence file makes it NON-EMPTY, so every existing `test -s`, `statSync().size`, `[ ! -s ]` and "manifest written" check can no longer distinguish a real run from a detached recorder that wrote nothing but a stamp. lazily-js added a stamp-with-no-records check and verified a 33-byte stamp-only file now refuses. lazily-cpp found the same thing in its CI workflow — the "manifest written" step tested `-s`, which a bare stamp satisfies — and changed it to count records BELOW the stamp. THE OTHER EIGHT MAY HAVE WEAKENED AN EXISTING EMPTINESS CHECK WITHOUT NOTICING. For each: find every emptiness or existence test over a now-stamped evidence file and confirm it counts records rather than bytes. lazily-zig is the one where this matters most and where it may already be fine by accident — its whole prevention was a `: >` truncation paired with `[ ! -s ]`, so if that pair still tests size it now passes on a stamp alone. lazily-rs is likely fine because its `conformance-manifest-reset` empties the files and its rungs fail closed on empty, but it stamps from the first writer after the reset, so verify the ordering. Probe each one the way js did: a stamp-only file must refuse, naming the file.
- [ ] [#lzrscorpusrootvacuous] [lazily-rs] The corpus-root guard reports `corpus-root guard OK: 0 default-root reference(s) across 0 file(s), all baselined` while the script's own header describes FORTY-FIVE baselined sites that should fail when they disappear. Found incidentally during #lzstalemanifest, byte-identical in the pre-change baseline log, so it is NOT caused by that work — but as the agent put it, the rung looks vacuous. This is the exact shape #lzvacuousrun exists to refuse: zero references across zero files is OK over nothing, and a guard whose population has silently gone to zero reports success by construction. DETERMINE WHICH IT IS, because both are plausible: either the 45 sites were legitimately removed and the header is stale prose (in which case the rung is dead and should be deleted or re-pointed), or the scan's discovery step has silently stopped finding files (in which case a real guard is inert). The discriminator is cheap: the header names what it baselined, so check whether those 45 sites still exist in the tree under any spelling. If the rung is live, it needs a zero-guard like every other magnitude in this family — 0 of 0 must fail, not pass.
- [ ] [#lzmakepfootgun] [docs, all ten] `make -p` BUILDS THE DEFAULT GOAL, and piping it into a short-circuiting reader corrupts state. lazily-rs lost a probe to this: `make -p | grep -m1 ...` started `make check`, ran `conformance-manifest-reset`, and then SIGPIPE'd when grep exited — SILENTLY TRUNCATING every file under build/ mid-analysis. The analysis command destroyed the evidence it was analysing. Use `make -pn` (dry run) or `make --eval` instead, and never pipe `make -p` into `head`/`grep -m1`/anything that closes the pipe early. Record this in the conformance docs beside the plant-probe method (#lzplantprobemethod), because every binding's Makefile is now load-bearing for the run-id protocol and inspecting those Makefiles is now a routine step — this trap is reachable by anyone auditing them, and its symptom (empty build/, guards failing as missing evidence) looks exactly like the bug under investigation rather than like self-inflicted damage.
- [ ] [#lzfailedgrepvacuous] [all ten, plus the agent-doc closeout instructions] `grep -c "result: FAILED"` IS VACUOUS IN AT LEAST FOUR BINDINGS, and I asked every agent in this session to use it as the closeout check. Proven, not suspected: lazily-dart deliberately failed a run (exit 1, tally `+14 -1`) and that string appeared ZERO times; lazily-gd's gdUnit4 never emits it; lazily-kt's Gradle/JUnit never emits it (0 on green AND would be 0 on red); lazily-js's `node --test` never emits it. So for those four the check has been reporting success by construction all night — it did not mask a real failure, because the exit codes were captured separately and were 0, but it contributed nothing and would not have caught anything. THE WORK: for each of the ten bindings, determine the real failure signal its runner emits and record it, then replace the generic grep with the per-binding signal. Known good substitutes: dart test's `+passed -failed` tally, gdUnit4's `Statistics: ... N failures`, JUnit XML failure/error counts for kt, `node --test`'s `ℹ fail N` for js, ctest's `N/M tests passed` for cpp. AND FIX THE INSTRUCTION SOURCE: the closeout guidance that propagates this lives in the agent-doc skill runbooks and in how I brief subagents — a check that is vacuous in 40 percent of targets should not be the standard one. The durable rule is the one every agent applied correctly anyway: capture $? explicitly and additionally assert the runner's own count, because an exit code can be masked by a pipe and a grep can be vacuous, but not both at once.
- [ ] [#lzastscanundercount] [lazily-rs, and any binding whose hygiene rung uses an AST parser] AN AST-ONLY SCAN SILENTLY UNDER-REPORTS, and lazily-rs measured the magnitude: a `syn`-based walk saw 39 of the 86 `as_bool()` sites in its test tree and reported the other 47 CLEAN, because `syn` DOES NOT DESCEND INTO MACRO TOKEN STREAMS and most fixture comparisons in that binding live inside `assert_eq!`. A rung that silently sees 45 percent of its target is worse than no rung, because it produces a green that reads as coverage. rs's rung now scans tokens as well as the AST. TWO MORE SPELLING TRAPS FROM THE SAME WORK, both of which an obvious implementation misses: `.and_then(Value::as_bool)` is a function PATH, not a method call, and that was the spelling of the one live site outside tests/; and lazily-kt's first pattern required a bare `()` so it missed `?: emptyMap<String, Boolean>()` with type arguments, caught only because it planted a deliberate near-miss. CHECK EVERY HYGIENE RUNG LANDED IN #lzsiblingrunnermasking against these three: does it see inside macros, does it match function paths as well as method calls, and does it tolerate type arguments and whitespace variation. lazily-js is likely safe (TypeScript parser, and it found that a REGEX mis-parses apostrophes in comments and then silently stops finding call sites — the same class one layer down). The general rule worth recording: a scan's floor must be pinned to the REAL population, which is what caught rs's undercount — its MIN_SANCTIONED_READS of 101 against an AST-only count of 39 would not have reconciled.
- [ ] [#lzplantprobemethod] [lazily-spec, docs] Record the plant-probe method and its three failure modes, discovered the hard way across #lzarrayelementsites and #lzflagcoercion. A plant-probe asserts a defect by planting it in a byte-verified scratch corpus and requiring GREEN BEFORE and a NAMED FAILURE AFTER. Three ways it lies, each hit by a different agent tonight. (1) A VACUOUS PLANT — lazily-rs wrote a `redeliver` gate onto scenarios[2], which carries neither `redeliver` nor `redeliver_applied_count`; the green meant nothing and briefly looked like a 15th defect. A plant must land on a scenario carrying BOTH the gate and the thing it gates. (2) A PLANT THAT REDDENS BEFORE — lazily-cpp had 3 of 16 do this and refused to present them as probes; lazily-dart's `handle_stable` reddened pre-fix because no step in the corpus re-mints a handle, so the coerced branch had nothing to pass against. A probe that was already red proves the fix changed nothing. (3) A PLANT MASKED BY A SIBLING RUNNER — see #lzsiblingrunnermasking; the same corpus edit is green in isolation and red in a full run, so probe scope must be stated. ALSO RECORD THE TWO ENVIRONMENT TRAPS: `grep -c "result: FAILED"` is ALWAYS 0 in lazily-gd because gdUnit4 never emits that string, so the standard closeout check is vacuous there — use `Statistics: ... N failures` plus an explicit `$?`; and lazily-kt's Gradle `:test` can read UP-TO-DATE under RTK, so a skipped suite reads as a passing one and `cleanTest test` is required. This belongs in lazily-spec/docs/conformance.md beside the existing perturbation guidance, because every binding's agent re-derived it independently tonight.
- [ ] [#lzcsreverserung] [lazily-cs] cs's block guard has NO REVERSE RUNG: a recorded bind whose digest no opened fixture declares fails nothing. 55 such digests exist today (runners binding sub-objects and similar), measured during #lzarrayelementsites. That is exactly why cs's 12 anti-spoof array elements could sit BOUND AND VERIFIED while being declared by nothing — the mirror image of every other binding's gap, invisible for as long as nobody counted. lazily-dart catches this because its ledger records PATHS, and lazily-rs catches it by classifying every bind the loader never declared (397 at the time, decomposing into 363 below an emitted block and 34 from its own self-tests) and REPORTING rather than failing, on the grounds that a hard equality there would be the MIN_BLOCKS defect in a new costume. Add the rung to cs in rs's shape: classify each undeclared bind as below-an-emitted-block, self-test-fabricated, or matching no corpus object at all, and report the decomposition. The third class is the one that should be zero and is the only one worth failing on. Do NOT add a bare count equality.
- [ ] [#lzpyelementdescend] [lazily-py] py does NOT descend into an emitted array element, which diverges from the spec and from the other six bindings. lazily-spec's blockSites walk descends into a block as well as past it, and says so deliberately: "No block in the corpus nests another today, so this changes no count — it is here so that the first one to do so is pinned by the change that adds it rather than by the change that later notices." py's reasoning is a real implementation constraint, not laziness: `instrument` hands the runner a TrackedBlock at that point, so a tracked key nested inside one would be DECLARED AND UNBINDABLE. But the consequence is that the first corpus fixture to nest a block makes py's derived count disagree with the audit's pinned row, and py would then be fixing `instrument` under time pressure rather than deliberately. Verified unobservable today: no corpus fixture nests a tracked key inside a tracked array, confirmed independently by lazily-rs (ran its twin with the array rule off and on, 771/661 then 783/673, no site lost) and by lazily-zig (cross-checked `{"expect":[[{"expect":{...}}]]}` against both halves, both saying 1 site). Fix `instrument` so a nested block can bind, then descend — or if that is genuinely impossible in py's shape, say so in the walk's comment so the next reader does not read the divergence as an oversight.
- [ ] [#lzsemtreesubset] [lazily-spec, design] collections/semtree_incremental.json's `expect_initial`/`expect_after` blocks name a SUBSET of the tree's nodes by design — scenario 1's tree carries `root a b a1 a2 b1` while `expect_initial` names only `root a b` — so a both-directions key-set check against the node set is unavailable without changing the corpus. Hit independently by lazily-go and lazily-cs during #lzarrayelementsites, and neither weakened a fixture nor reached for a ledger entry to dodge it, which was right. THE QUESTION IS WHAT "BOTH DIRECTIONS" MEANS FOR A FIXTURE THAT ASSERTS A SUBSET DELIBERATELY. The fixture-to-run direction is fine: every key it names must be read and compared. The run-to-fixture direction is the open one — `a1/a2/b1` are compared by nothing directly, so a defect that corrupted only those nodes would survive. Options worth weighing: name every node and lose the ability to express a partial assertion; add an explicit `nodes_not_asserted` companion so the omission is declared rather than implied; or pin the node-set CARDINALITY beside the subset so a node appearing or vanishing is caught even when its value is not. This is a lazily-spec decision because it changes the corpus contract for all ten bindings, and #lzgothreenames is blocked behind it for the run-to-fixture half.
- [ ] [#lzarrayelementdocs] [lazily-rs, lazily-kt] Two AGENTS.md/ledger-header surfaces still carry prose the #lzarrayelementsites work superseded, both deliberately left because the files belong to other panes. lazily-rs: AGENTS.md's #lzrsblockwalk paragraph still states the rule as "object-valued only, emitted and NOT descended into, with arrays descended" — which is now wrong in its second half — and quotes 771/661 in THREE places against a current 783/673. The undeclared-bind report also moved, 460 from 470 with below-an-emitted-block at 436 from 426, from the twelve new `Expect::sub("frame")` descents collapsing to ten distinct digests; the no-corpus-object half is unchanged at 34. lazily-kt: scripts/conformance-unbound-blocks.txt's header narrative says "outside a rung that saw 18 of the corpus's 737" against a current 749, and the kt agent correctly refused to touch that file at all because the instruction not to change it was unambiguous — so this needs a line-count-preserving comment fix in whichever commit next touches it. Use the hand-built-blob path (blob = HEAD + your hunk via hash-object -w plus update-index --cacheinfo, then a bare commit) rather than --only, and re-apply the hunk to the worktree copy afterwards so the owning pane's eventual wholesale commit does not revert it — lazily-kt and lazily-cs both did exactly that during this series and it is the only reason their doc changes survived.
- [ ] [#lzorderinganchors] [lazily-spec] scripts/check-assertion-ordering.py ORDERED_CHECKS anchors its ordering contract on the EXACT floor literals the bindings are now removing - regexes like 'assert accepted == 6', 'assert replayed == 12', 'assert.equal(accepted, 10)' for py/js/cs. lazily-py could not delete three of its floors this cycle and shipped them as DEAD ANCHORS, kept only to satisfy this guard, with the live constant-free assertion sitting above them. That is debt the guard created. Re-anchor on something stable that is not a value - a named helper call, or a marker the runner declares - so removing a constant does not break an unrelated ordering contract. Note the anchor must also be UNIQUE in the file (py tripped 'anchors are not unique' by quoting the literal in an explanatory comment). Repo: src/lazily-spec, then src/lazily-py to drop the dead anchors.
- [ ] [#lzktareapartition] [lazily-kt] check-conformance-coverage.sh declares REQUIRED_AREAS (27 areas) with no EXCUSED_AREAS complement, so the area list scopes the AUDIT and not the set the suite OPENS: kt opens 148 fixtures / replays 157 scenarios where deriving from the areas alone gives 147 / 151. lazily-spec's check-corpus-floors.mjs therefore declines to derive any floor for kt (it would otherwise have called an honest MIN_FIXTURES=148 unsatisfiable). lazily-cpp pairs the two arrays and its own guard enforces the exact complement, and there the derivation is exact. Add the EXCUSED_AREAS complement (or the equivalent partition assertion) so kt's floors become auditable from the corpus side. Repo: src/lazily-kt
- [ ] [#lzscenariofloormissing] [lazily-py, lazily-kt, lazily-gd] No MIN_SCENARIOS floor is declared at all - lazily-spec's check-corpus-floors.mjs reports this rather than failing, because lazily-kt deliberately retired MIN_FIXTURES for an area-partition assertion and that is a stronger invariant. But a binding with NEITHER is relying on nothing: py replays 151 scenarios and kt 157 with no positive-evidence floor under either, so a run that silently stopped replaying scenarios would report green. Derive each floor from the gate's own output, add it, and prove it exact by failing at n+1. lazily-gd is staged and may legitimately have nothing to floor yet - check before adding. Repos: src/lazily-py, src/lazily-kt, src/lazily-gd
- [ ] [#lzcireachsync] [lazily-rs, lazily-py, lazily-js, lazily-kt, lazily-go, lazily-cpp, lazily-cs, lazily-dart, lazily-zig] The ten copies of scripts/check-ci-reach.sh are no longer byte-identical. Before #lzgdcireach the nine all carried md5 da1444b38e93 / 570 lines — an invariant nothing enforces but which is plainly deliberate, since the script has no shared home (unlike check-assertion-ordering.py, which lives in lazily-spec and is invoked across the sibling boundary). lazily-gd 913edc8 added `make_invokes_ancestor`: a CI step running `make <ancestor>` reaches every target in that ancestor`s prerequisite closure, walked with the same `prereqs_of` the main closure uses. Without it a verbatim copy reported all six of gd`s gates unreachable, because gd`s CI is a single `make check` step. THE NO-OP PROOF IS ALREADY DONE, so this item is a port plus verification rather than a design question: the new script was run in each of the nine and produced output BYTE-IDENTICAL to the old one, exit 0 in all eighteen runs — because none of the nine invokes `make check` from a `run:` step (each runs the underlying commands per gate; the only real `run:` body in any of them containing the phrase "make check" is this guard`s own refusal text, which is the comment-vs-command trap its header warns about). So: copy gd`s version into the nine, re-run each binding`s `ci-reach` target and confirm output is unchanged, then `make check` per repo and CI. Worth deciding in the same pass whether nine-plus-one hand-synced copies should keep being hand-synced at all — the alternatives are a shared home in lazily-spec (which would make every binding`s own guard depend on the sibling checkout, a cost the current design deliberately avoids) or a guard that asserts the copies are identical. Note for whoever takes it: the file is 641 lines in gd now, and the ONLY behavioural difference is the two new functions plus one call site and the header sentence, so a diff against gd is the cheapest review. Repos: the nine above
- [ ] [#lzfollowupqueueorder] [process] Follow-up items filed by a drain cycle keep preempting the queue they were filed from, because `--backlog-add` prepends by default and the queue mirror honours that. Three consecutive cycles filed an item that became the very next head — #lzblocksitepin, then #lzexpectedkeyorder, then #lzgdorderingguard, then #lzgdcireach — each pushing #lzcppblockwalk one place further back, so the drain went depth-first into gd tooling while the three walk-widening items (cpp/kt/rs) and the three older items (#lzorderinganchors, #lzktareapartition, #lzscenariofloormissing) sat untouched. Prepending is the right DEFAULT — a follow-up from the turn that just ran is usually the most task-relevant work, which is exactly why the flag exists — but it is the wrong choice for a follow-up that is merely ADJACENT rather than more urgent, and nothing currently prompts the agent to make that judgement. Concretely: prefer `--backlog-queue-placement append` for a follow-up that is not more urgent than the existing head, and consider whether the agent-doc skill should say so at the point where it tells the agent to capture follow-up work (runbooks/respond.md, the backlog-capture rule) rather than leaving the default to decide. Worth checking whether the same asymmetry applies to `--backlog-add-after`/`--backlog-add-before`, which the queue mirror also honours. This item deliberately uses append on itself. Repo: src/agent-doc (skill/runbook text), or file against tasks/agent-doc/ if it is treated as a product change
- [ ] [#lzktbindpending] [lazily-kt] PHASE 2 of #lzktblockwalk, IN PROGRESS — 28 of 540 sites bound, pin re-pinned DOWNWARD 540 -> 512, 512 remaining across 13 areas. Migrate kt`s remaining bind-pending assertion-block sites onto AssertionKeys: the block is reached and the runner DOES assert its keys, but by direct indexing rather than through a tracker, so rung 0 cannot see the bind and every rung above it is scoped to blocks a tracker bound and therefore says nothing about these. They are enumerated per site in scripts/conformance-unbound-blocks.txt with reason class `bind-pending`, and the guard asserts that count as an EQUALITY failing in BOTH directions and derived from the RUN rather than the ledger, so a migration cannot land without moving the number in the same commit and cannot silently regress upward. FIRST PASS (lazily-kt 93420ff) bound 5 areas over 4 block shapes to prove the recipe generalises: ipc/root 1, familysync 3, protobuf 6, membership 9, message-passing 9. THE MEASURED GAP RATE IS THE REASON THIS ITEM IS BIGGER THAN ITS SITE COUNT: every one of those 28 blocks failed a higher rung once bound, and HALF needed a real fix rather than a routing change — 6 distinct defect classes, each a hole the narrow walk was hiding. `ordinary_snapshot_count` (6 blocks) compared against the LITERAL 0, so the key asserted the corpus against itself and a binding admitting a purposeless snapshot passed; `rpc.terminal_status` and `conflict` were PRESENT IN THE FIXTURE AND NEVER READ, the latter being the headline claim of terminal_conflict_fail_closed; `ignored_frame_indices` was asserted one way only; `target_epoch_bumped` was read but asserted only when TRUE, so a `false` upstream would have vanished the assertion; `*_after_frame_index` let the fixture`s own value steer which assertion to make instead of comparing against the run`s. Plus 7 object-valued keys consumed with no key-set check. If that rate holds the remaining 512 sites carry on the order of a hundred more real defects — but treat that as a measured rate from one sample, not a forecast: the easy classes may be exhausted first, so RE-MEASURE the per-area gap rate each pass and report it. REMAINING AREAS: collections 132, reactive-graph 88, stdlib 54, ingress 42, rateshape 33, coordination 29, windowing 23, egress 22, resilience 21, service 20, temporal 16, presence 16, lossless-tree 16. Take one or two per pass; delete exactly that area`s ledger entries and re-pin the equality down by exactly that count in the same commit; verify the guard fires on an off-by-one in BOTH directions. Every higher-rung failure a migration surfaces must be FIXED, never excused — a bound-but-failing block is a different state from an unbound one and must not be laundered back into the ledger. reactive-graph (88) is BLOCKED on #lzktreactivemodelfailures and should not be started until that is settled. Gradle proof discipline applies: JUnit XML timestamps, not task lines, and `--rerun-tasks` if a manifest may be stale. Repo: src/lazily-kt
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#lzambienttokenresolve] [operator-decision, agent-loop superproject] agent-loop/.mise.toml has `[env] _.source = ".env"`, and .env holds $(passage ...) REFERENCES for every credential — correct hygiene, untracked and gitignored. But mise RESOLVES those substitutions at shell startup, so the resolved cleartext sits in the ambient environment of every process spawned from this tree. Verified by presence check on 2026-09-12, values never printed: CARGO_REGISTRY_TOKEN (35 chars) and NPM_TOKEN (40 chars) are live in the environment right now. That is not a leak by itself — nothing here was found printing them, and the lazily-zig build specifically does NOT (see #lzzigenvsecretleak, measured) — but it means every tool in the tree is one env-dumping code path away from disclosure, and there are many such paths in build systems, crash handlers and CI log collectors. THE FIX IS SUBSTITUTION AT THE POINT OF USE: keep the refs in .env for documentation, but let the publish commands resolve them (`CARGO_REGISTRY_TOKEN=$(passage ...) cargo publish`) rather than mise resolving them into every shell. WHY THIS IS YOURS AND NOT MINE: changing .mise.toml alters the environment of every session started from this tree, and thirty peer sessions were live when this was filed — several mid-build. A change like that wants a quiet moment and someone watching, not a queue drain. SEPARATELY AND MORE URGENTLY: rotate CARGO_REGISTRY_TOKEN, NPM_TOKEN and PYPI_PUBLISH_TOKEN. Not because a build printed them, but because the resolved values passed through a subagent's tool output while it was diagnosing the (non-)leak, which puts them in that transcript permanently. Log shredding does not reach a transcript. crates.io has no Revoke button, only Regenerate — see reference_cargo_publish_token_passage.md.
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
