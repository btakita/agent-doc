---
agent_doc_session: d818612d-3614-4c43-b5ec-83923406aa46
resume: d6379dbb-42f5-4658-8da6-e0251c5eaa18
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
agent_doc_model_tier: high
queue: go
---

## Status

<!-- agent:status patch=replace -->

completed
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/44ae7f3a5c03b112ffe17aa5c85d000e3d79a6e9a4875e16442309ebf1c06a1c-20260809-231804.md`*

Compacted content:
- Archived 28 response topic(s): Phase 1; Phase 2; superproject pointers + verification; 25 more
- Prior summary/context: prior compacted content: Archived 49 response topic(s): authoritative statechart kinds and closed references; canonic...

### Re: #lzzigcanonrunner — canonical runners for the three collections CRDT fixtures — opus-5

> **Queue prompt:**
>
> do [#lzzigforkhlcpeer]


> **Queue prompt:**
>
> do [#lzzigcanonrunner]


Done, and it found a real wire defect.

**The runners.** `src/lazily/text_crdt_conformance.zig` and `src/lazily/seq_crdt_conformance.zig`, following `lossless_tree_conformance.zig`. 13 scenarios across the three fixtures replay for real — the runtime manifest and the scenario ledger both record every one, and `make check` is green (`132/150 fixtures OPENED`, up from 129; `144/144 scenarios REPLAYED`). All nine ops, all fifteen expect keys, and the clone/snapshot/delta/exchange/`new` step forms are handled; the three `KNOWN_UNCOVERED` entries are gone.

**The defect.** `SeqCrdt.contains` answered from the entry table, so a removed element — an LWW tombstone the merge algebra deliberately keeps — reported as present. lazily-rs answers `entries.get(id).is_some_and(|e| !e.deleted.value())`, and `seqcrdt_convergence.json` pins that reading through `not_contains_on`. The inline mirror in `seq_crdt.zig` had hedged around it:

```zig
try std.testing.expect(!ab.contains("b") or ab.get("b") == null);
```

which stays green under either reading — a mirror agreeing with the implementation it was transcribed from, which is the whole argument for opening the corpus. Both the library and the mirror are fixed.

**Proof the runners assert something.** Six implementation mutants, each rebuilt from a scratch backup rather than `git checkout` (which silently reverts the fix under test and changes what every later mutant is measured against):

| mutant | new runner | also caught by |
|---|---|---|
| M1 `TextCrdt.merge` adopts no unseen element | `textcrdt_convergence` red | 2 inline tests |
| M2 `SeqCrdt.merge` adopts no unseen entry | `seqcrdt_convergence` red | 1 inline test |
| M3 `SeqCrdt.contains` ignores the tombstone (pre-fix) | `seqcrdt_convergence` red | the tightened mirror |
| M4 `versionVector` omits delete stamps | `textcrdt_delta_sync` red | 1 inline test |
| M5 `moveAfter` lands before its anchor | `seqcrdt_convergence` red | 3 inline tests |
| M6 `gcWith` ignores origin references | **SURVIVES** | nothing |

M3 is the negative control for the fix: reinstate the pre-fix shape and the corpus reddens naming the exact reason.

Corpus perturbation over all 35 `expect` keys (scratch corpus copy, never the shared checkout) reddens 24. The 11 survivors are all symmetric mutations rather than vacuous assertions — reversing `texts_equal: [["a","b"]]` states the same claim, and a `not_contains_on` renamed to an id that never existed is trivially true — which is why the mutants above ask the question from the implementation side instead. Recorded rather than dropped.

**Coverage.** `check-coverage-claims.mjs` had flagged exactly these three rows as `~`-but-indistinguishable-from-shipped; they now have the evidence, so they are promoted. Its "indistinguishable" count drops 6 → 3. The `crdt` family roll-up is unchanged because Registers keeps Zig at `~`, so **no sibling README moved** — this was a 2-repo change, not the 12-repo commit `#lzpartialmarkadjudicate` warns about.

Pushed: `lazily-zig@43c5d00`, `lazily-spec@f2d395d`, superproject pointer `2d4776d`. CI is queued on both at the time of writing (`31342788665`, `31342790957`) — closing out on local evidence per the usual rule.

**Two findings filed rather than silently fixed**, both because the fix belongs upstream and reddens the family:

- `#lzspecgcreferencedtombstone` — M6's survivor. The `gc_collects_stable_leaf_keeps_referenced_tombstone` scenario deletes the LAST character of `"abc"`, so there is no referenced tombstone to keep despite the name. The rule is real and implemented everywhere; nothing in the corpus falsifies its absence.
- `#lzzigforkhlcpeer` — `SeqCrdt.fork` copies the source's whole `Hlc`, keeping the source's `peer`; rs forks with a fresh `Hlc::new(peer)`. Two separable questions ride on that line and the corpus pins neither, and on the second one (carry the causal position forward, or reset it) zig looks more correct than the reference. That needs a spec decision plus a discriminating scenario before either binding moves.

I left `AGENTS.md` in both repos alone — another session has it uncommitted, so the new modules are not documented there yet.

### Re: #lzzigforkhlcpeer — the fork clock question, decided and fixed across seven bindings — opus-5

The item asked me to decide (a) and (b) before touching anything. Both turned out to be decidable on evidence, and (b) was a live data-loss bug in six bindings rather than a matter of taste.

**The audit — a three-way split, not two.**

| binding | fork clock | stamp peer |
|---|---|---|
| rs (reference), js, kt, py, cs, cpp | **RESET** | new |
| go, dart | CARRY | new |
| zig | CARRY | **source's** |

**(a) zig's peer, fixed first.** Carrying the source's whole HLC carried its *peer*, and the peer is the stamp's final tiebreaker. Two replicas stamping under one peer id can mint the identical `(wall, logical, peer)`; LWW adopts only on `.gt`, so neither adopts and they diverge **permanently**. The regression test failed on the *convergence* assertion (a=55, b=99) before the fix — not on which value won. `lazily-zig@c7cfa5a`.

**(b) RESET is a bug, and I proved it rather than argued it.** A fork has observed everything the source holds, so a fresh clock lets its next local op — with any backwards-skewed `now`, which is the entire reason HLCs exist — mint a stamp *behind* state it already holds. LWW then rejects the fork's own write. Reproduced live against `lazily-js`:

```
a.insertBack('x', 1, 100)   // stamp (100, 0, 1)
b = a.fork(2)               // RESET -> clock (0,0)
b.setValue('x', 99, 50)     // mints (50,0,2), not > (100,0,1)
b.get('x') === 1            // the fork's own write silently vanished
```

`cell-model.md` says nothing normative about fork clock semantics, so there was no authority to appeal to — which is why the split survived nine bindings.

**Fixed everywhere.** Six agents, one per repo, each writing the test first and confirming it red before fixing. All seven gates exit 0:

| repo | commit | gate |
|---|---|---|
| rs | `c678ff1` | `make check` 0 (841 ok, incl. loom + Lean + benchmark budgets) |
| js | `2291748` | `make check` 0 |
| kt | `a63356c` | `make check` 0 (+ `--no-daemon --rerun-tasks`: 447 tests really ran) |
| py | `4e0b04d` | `make check` 0, 759 passed, CI green |
| cs | `a8b05e7` | `make check` 0, CI green |
| cpp | `6b414b9`, `ee5f665` | `make check` 0, CI green |
| zig | `c7cfa5a` | `make check` 0 |

Superproject pointer `9a2a9c4`. go and dart needed no change.

**Three things the fan-out turned up that I did not brief for:**

- **lazily-py had no HLC at all.** Its module docstring claims "each stamped by an HLC"; the stamps were bare `int`s with no peer, local writes adopted on `>=`, and `SeqCrdt` had no `fork` — the conformance runner synthesized one with `clone()` then `.peer = 2`. So py had *both* failure shapes, the second reachable without even sharing a peer id, since a bare int has no peer to differ on. Now a real `SeqStamp`/`_SeqClock`.
- **cpp's `make wasm` was already red on `main`** — the corpus gained a Registers fixture and `WASM.md`'s count was stale (CI 31340403147 failed on it *before* this work). Regenerated properly with `make wasm-matrix`, not by hand.
- **rs's test-only `clone_state`/`clone_state_as` helpers carried the same defect** and now delegate to `Clone`/`fork`.

**What I did not do, and why.** The cross-binding pin. All nine now agree, but nothing in the corpus enforces it: every fork in `seqcrdt_convergence.json` is followed by an op whose `now` exceeds the source's `last_wall`, so `send` resets the logical counter regardless of which clock the fork started from. Each binding carries its own regression test; the *family* agreement is unpinned. Filed as `#lzspecforkclockfixture` with the two scenarios to add — and worth doing soon, because it is uniquely free right now: both cases pass on all nine today, so unlike the usual new-fixture change it reddens nobody.

Also filed `#adjshashprivatefield` — agent-doc's `coined-id-pre-tool-use` hook blocks edits containing JS private class fields (`#entries`, `#hlc`) as coined ids; only C-family files are sanitized. It belongs in `tasks/agent-doc/agent-doc-bugs2.md`, so it is recorded here only to be moved, not worked.

One standing caveat: the running `agent-doc` binary is stale (missing `bdee55ba4`), and `src/agent-doc` has ~150 uncommitted files from another session, so I did not run `make install` — building another session's half-finished refactor into the live binary is not mine to do.

### Re: #adjshashprivatefield — moved to the owning document, not worked here — opus-5

This item was filed to be *relocated*, and that is what I did — but I verified the bug at the source first rather than forwarding a subagent's claim unchecked.

**Confirmed, and more precisely than it was reported.** Two pieces of code have to agree for a JS private field to be blocked, and they do:

- `coined_id_scan_text` (`agent-doc-hooks-io/src/coined_id_pretooluse.rs:213`) returns the payload **unsanitized** unless `is_c_family_target` matches, and `C_FAMILY_EXTENSIONS` lists only `c/cc/cpp/cu/cuh/cxx/h/hh/hpp/hxx/inc/inl/ipp/m/mm/tpp`. No `js`/`mjs`/`cjs`/`ts`/`tsx`.
- `extract_tags` (`agent-doc-turn/src/coined_ids.rs:28`) has exactly one relevant guard: a tag must not be *glued to a preceding word*, tested as "previous char is alphanumeric, `_`, or `-`". For `this.#entries` the previous char is `.`; for an indented `  #hlc;` declaration it is a space. **Both pass the guard**, so both yield tags.

So it is not a fuzzy false positive — the two conditions compose exactly, and any edit to a modern JS class is blocked while a document is bound.

**Filed as `#hookhashjsprivatefield` in `tasks/agent-doc/agent-doc-bugs2.md`.** That document already carries `#hookhashfalsepositive` for the C-preprocessor instance of the same guard, so I did not open a duplicate: the new item is scoped to what the C-family fix *left behind*, with the two file:line sites and both a narrow fix (extend the sanitizer to JS/TS class-member position) and the broader one that item already proposes. Worth noting this is the **second language** to hit the same guard, which is an argument for scoping it to prose contexts rather than enumerating extensions one incident at a time.

**Why I did not fix it in this session.** `src/agent-doc` currently holds ~150 uncommitted files from another session's in-flight crate-split refactor (its own `session-check` has been warning about partial staging all evening). Editing the hook there would mean running `make check` against someone else's half-finished tree, where I could not tell my breakage from theirs — the exact "local check green while HEAD is broken" trap. The fix is small and belongs on the agent-doc bugs queue, where that session's coordination lives.

Housekeeping: the exchange is large (109 lines, session-accretion `warn`) but the queue is active, so I am not pausing to compact.

### Re: #lzspecforkclockfixture — the fork-clock rule is now pinned across all nine bindings — opus-5

> **Queue prompt:**
>
> do [#lzspecforkclockfixture]


Landed: the normative statement in `cell-model.md`, two discriminating scenarios in `seqcrdt_convergence.json` (6 → 8), and per-binding proof that every runner actually replays them.

**The scenarios.** Both use only vocabulary the fixture already had — `seed.inserts`, `fork`, `set_value`, `merge`, `get_on`, `len` — which is what made this free: no runner needed new dispatch.

- `fork_carries_the_clock_so_a_backwards_skewed_write_survives` — insert at `now=100`, fork to peer 2, `set_value` at `now=50`. Under a reset clock the write is *rejected*, not overwritten.
- `fork_stamps_with_its_own_peer_so_equal_wall_edits_converge` — both replicas write at the same `now`. Under a shared peer id the stamps tie and neither adopts.

**Verified, not assumed.** Every one of the nine reported its own runtime scenario ledger naming both ids, and — because a ledger entry proves a payload was *read*, not that assertions *bite* — each also ran a mutation probe. Uniformly, resetting the clock reddens only the first scenario and carrying the peer reddens only the second, so each half of the rule is independently pinned. Probes ran against scratch corpus copies; the shared checkout was never perturbed.

Two runners needed work, both in their **census** rather than their replay: `lazily-cs` asserted `scenarios.Count == 6` (it failed with `Expected: 6 / Actual: 8`, which is itself proof the loop visited all eight) and `lazily-cpp`'s `MIN_SCENARIOS` floor was stale. `lazily-rs` also corrected a comment I wrote this morning claiming the corpus could not reach these shapes — true when written, false the moment the fixture landed.

**I got the publish order wrong, and CI said so.** I pushed the bindings before the corpus. Every binding's CI clones lazily-spec from *published main* while local verification uses the sibling working tree, so `lazily-cs` went red with the inverse assertion (`Expected: 8 / Actual: 6`) and `lazily-cpp` had to pin its floor to the published 144 instead of the local 146. Two agents diagnosed it independently, which is the tell that it is a process gap and not a one-off. Fixed by pushing `lazily-spec@f3246a1` first, then re-running: **cs, dart, cpp and zig CI are now green**, and cpp's floor is raised to 146 in a follow-up (`b9c1ed8`) now that the scenarios are genuinely published. `lazily-rs` CI is still running its long gate (loom + Lean + benchmark budgets) — recorded as pending, closing out on local evidence.

Filed `#lzspecpushbeforebindings` so the ordering rule gets written down where it will be read, and ideally made mechanical.

**Commits:** `lazily-spec@f3246a1`, `lazily-rs@45c855e`, `lazily-dart@f75994e`, `lazily-cs@2471eba`, `lazily-cpp@5d54573`+`b9c1ed8`; superproject `d2d84d4`, `cf1fac8`. js, go, kt, py and zig needed no change — reported as genuine no-ops with ledger evidence rather than dressed up as fixes.

With this, the `#lzzigforkhlcpeer` thread is closed end to end: a defect found by opening a corpus, fixed in seven bindings, and now pinned by the corpus that could not previously see it. Exchange is large (session-accretion `warn`) but the queue is active, so I am not pausing to compact.

### Re: #lzspecpushbeforebindings — the ordering rule, written down and made mechanical — opus-5

> **Queue prompt:**
>
> do [#lzspecpushbeforebindings]


Two halves, both landed in `lazily-spec@f18c03b`.

**The rule**, in `docs/conformance.md` § Publishing a corpus change. It states the asymmetry that causes the failure — local verification resolves the corpus through the sibling **working tree**, every binding's CI **clones published `main`** — and gives the order that works, with last cycle's two casualties as the worked example: `lazily-cs`'s `Expected: 8 / Actual: 6` and `lazily-cpp` pinning its floor to 144 and then raising it in a second commit. It ends on the invariant worth remembering: *a count or floor in a binding must describe what CI's clone guarantees, never what your working tree happens to hold.*

**The mechanism**, `scripts/check-corpus-published.mjs`, wired into `make check`. It fires at the moment the rule applies — when `conformance/` is ahead of `origin/main` — names each drifted fixture with its state, and says what CI will replay instead.

It **advises rather than gates**, deliberately. A corpus ahead of `main` is the normal state while a fixture change is being authored, so failing there would gate the ordinary workflow to catch a mistake made in a different repo. The gate that could fail closed would have to live in each binding, and an unpublished corpus is a legitimate local state there too. The skip path says *"SKIPPED — this run did NOT establish that the corpus is published"* rather than degrading into a pass.

**Exercising it found a false OK.** I ran all three drift branches instead of reading them, and the untracked leg did not exist until that run: `git diff` does not report untracked files, so a **brand-new fixture** — the change most likely to move fixture counts, scenario counts and coverage floors at once — reported a clean "corpus publication OK". Added `git ls-files --others` and re-ran; it now reports `UNTRACKED — git has never seen this file`. Committed-but-unpushed and uncommitted both report correctly, and a clean corpus reports OK. The probes were reverted and the corpus is byte-identical at `f3246a1`.

`make check` green. Superproject `5674b8a`.

**Also:** `lazily-rs` CI, the one item left pending last cycle, came back **green** — so every binding touched by the fork-clock work is now green on published `main`. `lazily-spec`'s own CI for this commit is pending at the time of writing.

One thing I could not do: put a pointer in `AGENTS.md`, the file a harness auto-loads. It carries ~30 lines of uncommitted edits from the concurrent session rewriting the shared code-navigation block across every repo, and `git commit --only -- AGENTS.md` commits working-tree content, which would have swept their in-flight work into an unrelated commit. Filed as `#lzspecagentsmdpointer`. In practice the mechanical check is the stronger placement anyway — it prints on the exact action rather than waiting to be read.

### Re: #lzspecagentsmdpointer — done, and the "blocked" framing was wrong — opus-5

> **Queue prompt:**
>
> do [#lzspecagentsmdpointer]


I filed this last cycle as blocked: `AGENTS.md` carries ~30 lines of a concurrent session's uncommitted code-navigation rewrite, and `git commit --only -- AGENTS.md` commits the file's whole **working-tree** content, which would have swept their in-flight work into my commit.

That was the right diagnosis and the wrong conclusion. `--only` protects you from other *staged paths*, not from other *edits inside the same file* — but the index entry can be built by hand:

```
head   = git show HEAD:AGENTS.md          # NOT the working tree
staged = head with my section inserted
blob   = git hash-object -w --stdin < staged
git update-index --cacheinfo 100644,<blob>,AGENTS.md
git commit          # no paths, no -a — both would re-read the working tree
```

Verified in both directions before pushing: `git diff --cached` showed only my 14 insertions, `git diff` showed only their 30-line edit. After the commit, `git show HEAD:AGENTS.md` still contains their **pre-edit** tsift block (`v=0.1.77`) while the working tree has their new one (`v=0.1.80`) plus my section. Their work is exactly where they left it, uncommitted.

The section sits beside **Commit & Push** — the decision it actually modifies — and deliberately far from their hunk so the two never merge into one diff. It states the asymmetry in three sentences and points at `docs/conformance.md` for the worked example, since the auto-loaded file should name the rule, not restate it.

`make check` green. `lazily-spec@ccf0160`, superproject `60d661803`.

Worth keeping: this is the exact inverse of the shared-index rule I already follow (`--only` to avoid another pane's *pre-staged files*). Same tool, opposite failure. Saved as a memory.

Queue note: exchange is at 175 lines / 5 sections (session-accretion `warn`), but the queue is active so I am not pausing to compact. Next head is `#lzspecgcreferencedtombstone` — and unlike the last two spec changes, that fixture is expected to redden bindings, so the corpus goes first per the rule just written.

### Re: #lzspecgcreferencedtombstone — the GC blind spot is closed, and M6 now dies — opus-5

`gc_collects_stable_leaf_keeps_referenced_tombstone` claimed both halves in its name and tested one. It deletes index 2 of `"abc"` — the **last** character — so nothing references that tombstone as a left origin, and a collector that ignored origins entirely collected the same single element and stayed green. That is why mutant M6 survived four cycles ago while five sibling probes each reddened.

`gc_keeps_a_tombstone_that_is_still_a_left_origin` deletes an **interior** character instead, so the survivor `c` still names the tombstoned `b` as its origin and the collection must return 0.

**M6 now dies.** Re-running the identical mutant against zig: `expected 0, found 1`. And every binding reproduced the same discriminating asymmetry — under the mutant the **old** scenario passes and only the **new** one fails:

| | probe result |
|---|---|
| rs, js, py, go, kt, dart, cs, cpp, zig | old gc scenario PASS · new gc scenario FAIL |

kt's evidence was the sharpest: its ledger from the *failing* run contains the old scenario, contains the new one where it died, and is **missing** the scenario after it — so the abort point is visible in the ledger itself.

**Ordering, this time correctly.** I pushed `lazily-spec@34caf9c` first — and the advisory I wrote last cycle fired on its first real use, telling me to do exactly that before I touched a binding. No binding's CI ever saw a stale corpus. All six repos that changed are green in CI.

**No library changed anywhere** — all nine already implemented the rule. What moved was bookkeeping: cs's exact census (`Expected: 10 / Actual: 11`, with its step and assertion floors recomputed to exact totals and *proved* exact by raising each one further and watching it fail), floors in rs/js/cpp, and dart's vendored mirror.

**What I deliberately left unpinned.** Cascade depth splits the family roughly evenly — rs/js/dart/kt iterate to a fixpoint within one call; py/cs/cpp/zig run a single pass and leave the newly-unreferenced tombstone for the next. Both conform: "bottom-up" describes the order reclamation is safe in, not how much one call must do, and `#lzspecgcdefer` already places collection timing outside the contract. I wrote that into `cell-model.md` rather than leaving it implicit, because the next person reading "reclaimed bottom-up" would otherwise be entitled to "fix" four bindings toward fixpoint for no reason. The fixtures assert *which* tombstones survive, never how many passes it took.

**One finding filed.** rs's floors have drifted far below reality — `MIN_SCENARIOS` 137 against 162 observed, `MIN_FIXTURES` 131 against 144. A 25-scenario gap means 25 scenarios could stop replaying with the guard still green, which is the failure these floors exist to prevent. js has the same shape (130 vs 153). The agent correctly declined to close the gap without knowing whether CI's replay set equals the local one. Filed as `#lzscenariofloordrift`.

Commits: `lazily-spec@34caf9c`, `rs@01db593`, `js@9daf817`, `dart@0d040fe`, `cs@7b5ceb8`, `cpp@870a4a6`; superproject `9b190af`. Exchange is at 204 lines (session-accretion `warn`); queue is active so I am not compacting.

### Re: #lzscenariofloordrift — every floor in the family is now exact — opus-5

The blocker I filed this under ("nobody has confirmed the local replay set equals CI's") dissolved in one command: **CI prints its own count**. rs's job logs `scenario coverage OK: 162/162` and `conformance coverage OK: 144/150`, matching local exactly. Same for js, dart, cs, cpp. For zig/go/kt/py the last runs predated two corpus additions, so I re-triggered CI to re-clone the current published corpus rather than pin to numbers I had not seen CI confirm.

**The drift was worse than the item said.** It named rs and js; zig was the worst in the family:

| | scenarios | fixtures | blocks / other |
|---|---|---|---|
| zig | 91 → **147** | 111 → **132** | 25 → **31** |
| rs | 138 → **162** | 131 → **144** | 20 → **30** |
| js | 130 → **153** | 132 → **141** | 20 → **32**, plus 940 → **1006** and 130 → **206** |
| dart | 131 → **149** | 130 → **138** | — |
| cs | 148 → **165** | 141 → **145** | 25 → **33** |
| go | 137 → **147** | 131 → **141** | — |
| py | none | 132 → **139** | 20 → **31** |
| kt | none | none | areas 22 → **26** |
| cpp | 147 (already exact) | 127 → **137** | — |

zig's floor tolerated **56** scenarios going dark silently.

**The cause was a convention the comments themselves taught** — raise a floor by however many a change adds, keep the pre-existing margin — so every gap could only widen. cs's comment said it outright: keep the floor *"slightly below the real number so ordinary corpus churn does not trip it."* Every comment now states the replacement rule and names the old one as the bug.

**Each floor was proved, not merely passed:** set to N+1, guard must fail naming that floor. All nine repos, every floor, with the message recorded. That discipline caught a mistake of **mine**: I told the js agent to pin `MIN_OBJECT_VALUED_KEYS` to 196, but the guard's subject is the *denominator* (206). A 196 pin would have passed its own +1 probe while bounding nothing — 206 could have fallen to 197 with the guard green. Real drift there was 76, not 66. The output line now names which number it bounds.

**Two floors printed no number at all** — kt's area count and py's declared-block count — so they could not be re-pinned from CI by anyone. Both now print one; a floor whose value nobody can read drifts by construction.

All nine repos green in CI on their pinned commits, each run re-reporting the pinned figures. Commits: `zig@10fef17`, `rs@fee1f45`, `js@ca0d25e`+`a72868a`, `dart@1d53716`, `cs@590139f`, `cpp@16f7d35`, `go@5b89511`, `kt@eff8c0d`, `py@c935ef7`; superproject `4f20d6e` plus the js follow-up.

Also filed the cs `AGENTS.md` prose item — it documents stale floor values *and* still preaches "calibrated below the observed", the very convention this removed. Blocked the same way as before by the concurrent session's uncommitted edits, so the item carries the single-hunk staging recipe.

Process note: the coined-id hook blocked my first attempt at this response because the new backlog id appeared in the text before the item existed. Correct catch — I filed the item, then responded. Exchange is at 228 lines (session-accretion `warn`); queue is active so I am not compacting.

### Re: #lzpartialmarkadjudicate — three marks adjudicated, zero promoted — opus-5

The item warned against bulk-promotion. Adjudicated one at a time on binding-specific evidence, **none of the three was promotable** — every one turned out to mean exactly what a tilde is supposed to mean.

**GDScript on reactive-graph → the gap IS pinned, by fixtures the row did not cite.** lazily-gd's own guard lists `churn_returns_to_baseline` and `recycled_id_inherits_nothing` as uncovered — its runner lacks the bulk fanout/churn/dispose ops and real id recycling (`#lzgdvocab`). I checked all nine other bindings: **only** gd lists them. So the row now cites both, which makes gd's mark *verifiable* rather than merely unverified, and no other binding's mark moves.

**Zig on reactivemap-core → the gap is real and no fixture can pin it.** The binding had already written the answer in the header of `collections_family_conformance.zig`: its map is not graph-backed, so entries are cached values rather than reactive nodes, and `invalidates` is asserted against three **version counters** instead of live graph edges. That satisfies every assertion the corpus can express while being strictly weaker than lazily-rs's graph-edge check. It just had nowhere machine-readable to say so.

**C# on reliable-sync → not a C# fact at all.** The row is `~` for **all nine** bindings. The cited fixtures pin the coalescing/outbox and lease-eviction half; the resync coordinator plus OR-set/LWW liveness has no canonical fixture, so no ledger can separate a binding that finished it from one that did not. C# was flagged only because it happens to replay both cited fixtures cleanly.

**The third case had nowhere to live**, which is why it kept resurfacing as a review nag every run. Rows now carry `partial_reasons`, keyed by language, and the guard enforces them **in both directions**:

- an indistinguishable `~` with **no** reason now **FAILS** (it was previously a printed suggestion)
- a reason attached to a mark that is no longer `~` **FAILS as stale**

Both probes run, both fail with the intended message. A reason that could only be printed and never falsified would be the same folklore this guard exists to replace — the same discipline as `excuseKey` and the prose-key discharge.

The review list is now **empty** (was 3); the guard reports 2 declared partials instead, and prints them on every run so the claim stays in front of whoever next touches the row.

Worth stating plainly: this row had been carried as a review item for a long time on the assumption it was mostly promotions waiting to happen. It was the opposite — the marks were right and the *vocabulary* was missing.

`lazily-spec@7005876`, superproject pointer committed. `make check` green. Exchange is at 260 lines (session-accretion `warn`); queue is active so I am not compacting.

### Re: #lzcsagentsmdfloorprose — the prose now matches the scripts, and the bad advice is gone — opus-5

> **Queue prompt:**
>
> do [#lzcsagentsmdfloorprose]


Two problems, and the second mattered more than the numbers.

**Stale values:** the paragraph documented `MIN_FIXTURES` (139) and `MIN_SCENARIOS` (134) against real values of 145 and 165. Now correct, and `MIN_BLOCKS` (33) named too — it existed in the script but appeared nowhere in the prose.

**Bad advice:** it described the scenario floor as *"calibrated below the observed 139"* — precisely the convention `#lzscenariofloordrift` removed. That sentence is the mechanism, not a symptom: a floor deliberately set under reality has stopped guarding, because the drop it exists to catch fits inside its own margin. 17 scenarios of slack had accumulated here. The paragraph now says a floor is the exact count a completed CI run reports, never old-value-plus-your-delta, and to prove it by setting it to N+1 and watching the guard fail. It also names its own former advice as the bug, so the next reader sees the correction rather than silently re-deriving the same convention.

**Unblocked with the single-hunk technique**, second use. `AGENTS.md` still carries the concurrent session's uncommitted code-navigation rewrite at lines 239-268; my hunk is at 50-66, well clear of it. Staged a blob of HEAD content plus my change, committed the index with no paths — verified before pushing that `git diff --cached` held only my 10 lines and `git diff` only their 30, and after committing that their edits are still dirty and untouched.

One correction I caught myself: the first splice left a stray three-space indent on a continuation line. Re-derived the staged blob from an explicit old→new replacement instead of index arithmetic, and confirmed the line is gone from the working tree.

`make check` green. `lazily-cs@0e603c0`, superproject pointer committed.

Process note: preflight raised `queue_stall_detected` — "prior cycle committed with 7 drainable heads but the loop neither continued nor recorded a valid stop reason". That is an artifact of how I am pacing this drain: each cycle ends its turn and re-enters through a scheduled wakeup, so from the binary's view the turn stopped without continuing in-turn. The queue has in fact drained 8 items in a row. Worth knowing the signal cannot distinguish "paused between cycles by design" from "stalled", but nothing is stuck.

Exchange is at 283 lines (session-accretion `warn`); queue is active so I am not compacting.

### Re: #lzzigspecdiroption — runtime corpus root, and the skip trap closed structurally — opus-5

Both roots were comptime constants — `CONFORMANCE_ROOT` in `conformance_json.zig` and `SPEC_DIR` in `reactive_map.zig`, the latter `++`-concatenated at four sites. Both now resolve through `LAZILY_SPEC_CONFORMANCE_DIR`, the variable the sibling bindings and this repo's own coverage guard already use. Default unchanged; `SPEC_DIR` is gone.

**The compounding trap was wider than the item predicted.** It flagged `specFixturesPresent()` in one file. There are **eleven** presence probes in this repo with the same shape:

```zig
fn specFixturesPresent() bool { _ = read(...) catch return false; }
```

Every one converts *any* error — including "the corpus you named is not there" — into `error.SkipZigTest`. So returning an error from the read is not enough: the error is swallowed by exactly the code the fail-closed rule exists to protect. I found this because my first implementation *did* return an error, and the nonexistent-directory probe still produced **11 skips** instead of failing.

So the check sits in `specReadFile`, the one point every corpus read funnels through, and **panics**. A panic cannot be caught into a skip, so the guarantee is structural rather than something eleven call sites must each remember. That also let me simplify the per-site handling back to plain `catch return null` / `catch return false`, with a comment saying why that is safe *here*.

**Verified in four states rather than argued:**

| | result |
|---|---|
| default, no override | 523 pass |
| override → byte-identical copy | 523 pass |
| override → copy with one perturbed expectation | **1 failed** |
| override → nonexistent directory | **panics, 0 skips** |

The third is the one that matters: it proves the option changes which **bytes** are read, not merely which path gets printed. The fourth is the trap, now closed.

**The self-test gained the sibling it never had.** The existing `an absent corpus loads as null` test reads a deliberately-missing path, so it now opts out via `absence_probe_mode` — otherwise the whole suite aborts under an override, which is precisely when a probe needs it. But asserting *only* the null branch is how the trap survived in the first place: absence has two provenances and the suite pinned one. There is now a test for the fatal branch too.

`make check` green, 147/147 scenarios. `lazily-zig@6e5d677`, superproject pointer committed.

This unblocks `#lzperturbaudit` for zig — the per-family perturbation sweep no longer needs to edit and restore a constant, and can no longer be fooled by a typo'd path. Exchange is at 306 lines (session-accretion `warn`); queue is active so I am not compacting.

### Re: #lzktcorpusnotgradleinput — the corpus is now a declared input, proved on a warm daemon — opus-5

> **Queue prompt:**
>
> 📌 do [#lzktcorpusnotgradleinput]


Both halves were exactly as the item described, and `tasks.test { }` declared neither.

**The corpus is now an input.** Editing a fixture left `test` UP-TO-DATE — Gradle saw no declared input change, skipped the task, and reported GREEN having replayed nothing. That is the worst failure available to a corpus guard: every rung above it reasons about fixtures the suite *opened*, and a suite that never ran opened none, so nothing else can contradict it.

`optional(true)` is deliberate — a checkout without the `lazily-spec` sibling is a legitimate local state the tests already skip on. A **missing** corpus must not fail configuration; only a **stale result** must not be reused.

**`LAZILY_SPEC_DIR` is now passed explicitly.** The test JVM inherited the *daemon's* environment, and the daemon outlives the shell that started it, so one launched before the variable was exported kept serving the old path to every later run. Declaring it also puts it in the task's input fingerprint.

**Proved against the definition of done — warm daemon, no flags:**

| | result |
|---|---|
| unchanged corpus | `Task :test UP-TO-DATE`, green |
| one assertion key flipped in a scratch corpus | `Task :test FAILED`, naming the test |
| `LAZILY_SPEC_DIR` switched back to canonical | **re-runs**, does not reuse the scratch result |
| unchanged again | `UP-TO-DATE` |

The first and last rows matter as much as the second. Making a task always dirty would also turn the fixture edit red, and would be the wrong fix — it would trade a false green for a permanently cold cache. Keeping UP-TO-DATE on an unchanged corpus is what shows the fingerprint is *correct* rather than merely always-invalid. The third row is the environment half: a different corpus produces a different fingerprint instead of a reused verdict.

`make check` green. `lazily-kt@9fdcd9a`, superproject pointer committed.

Third item in a row from the same family — a guard that answers "nothing was examined" with the same signal as "nothing was wrong". zig's skip trap, and before it the floors set below reality. Worth noticing the shape: in each case the mechanism that hid the gap was one someone had deliberately added for a good local reason (skip without a sibling, margin against churn, incremental build caching).

Exchange is at 337 lines (session-accretion `warn`); queue is active so I am not compacting.

### Re: #lzledgeragreementaudit — nine bindings audited, two real splits found, both closed — opus-5

> **Queue prompt:**
>
> 📌 do [#lzledgeragreementaudit]


Audited all nine non-cs bindings for the split lazily-cs died of: a `KNOWN_UNCOVERED` entry in the shell guard that a fixture-**enumerating** test host knows nothing about.

**Five of the nine already carried a remediation commit for this item** — `rs@390ffa1`, `dart`, `kt@2ed176d`, `cpp@d93789a`, `zig@17db407`. That is why the item was `🚧`. The audit's job was the remaining question: is the class actually closed everywhere, or did the earlier sweep fix the instances it happened to look at?

| binding | enumerating host? | verdict |
|---|---|---|
| rs | `schema_compliance.rs` (corpus root, name-filtered) | SAFE — in-host ledger + fail-closed |
| go | reactive-graph glob, **drift assertion only** | SAFE |
| js | none — three readdirs are drift/count checks | SAFE |
| py | reactive-graph glob, drift assertion only | SAFE |
| dart | reactive-graph `listSync`, pinned before any read | SAFE |
| kt | `Files.list(reactive-graph)`, pinned before any read | SAFE |
| cpp | reactive-graph + statechart, cross-check only | SAFE for shapes — **but see below** |
| zig | reactive-graph `iterate`, drift detector only | SAFE |
| gd | none — `REPLAYED` names 19 ids | SAFE for discovery — **but see below** |

The pattern that makes eight of them safe is the same one, and it is worth naming: **the enumeration is a tripwire, not a drive list.** Every binding that lists a directory asserts set-equality against a hardcoded fixture set *before reading a byte*, then replays from the hardcoded set. So a new upstream fixture reddens the listing check rather than reaching a dispatch that has never been taught it. Nobody has to remember to handle the unknown shape, because the unknown fixture never gets that far.

Both remaining gaps were **the same defect at a rung the earlier sweep did not look at**, so this was not a formality.

---

**cpp — an entire area was invisible to every rung at once.**

`egress/` ships four fixtures. It was absent from `REQUIRED_AREAS`, absent from `KNOWN_UNCOVERED`, and absent even from the prose comment that lists the other deliberate omissions. The completeness loop iterates `REQUIRED_AREAS`, so those four fixtures were never demanded, never excused, never counted — and the guard printed `OK`. Every sibling binding names them in `KNOWN_UNCOVERED`; cpp alone had no ledger entry that could go stale, which is precisely why nothing reported it.

This is the audited defect one level up: not two ledgers disagreeing, but **one ledger with no counterpart at all**. What stood in for the counterpart was a comment — and a comment cannot be falsified by a run. Three areas were being carried that way.

The complement of `REQUIRED_AREAS` is now an array, `EXCUSED_AREAS`, and the two must **partition** the corpus. Four rungs, each driven against a scratch corpus rather than argued:

| probe | result |
|---|---|
| new upstream area (`telemetry/`) appears | `ERROR: conformance area 'telemetry' is in neither REQUIRED_AREAS nor EXCUSED_AREAS` |
| egress fixtures removed from `KNOWN_UNCOVERED` | 4 × `exists on disk but is never replayed` |
| an excused area the suite DID replay from | `ERROR: EXCUSED_AREAS lists 'egress', but the suite DID replay from it` |
| a listed area deleted upstream | `ERROR: area 'egress' is listed here but is not in the canonical corpus` |

And the control that makes those numbers mean something — **the pre-fix guard, same corpus, same manifest: exit 0, `egress` mentioned zero times. Exit 0 again with the brand-new area present.** The rung did not get stricter; it did not exist.

`agent-doc` (2), `egress` (4), and `signaling/frames.json` moved out of prose into `KNOWN_UNCOVERED` with their reasons; `signaling` joins `REQUIRED_AREAS` because `anti_spoof_session.json` is replayed. `make check` OK — 137 fixtures across 24 areas, 147/147 scenarios. `lazily-cpp@b575c3a`.

---

**gd — the shape dispatch failed open, reachable by an upstream edit.**

`run()` read `shape` with a `"steps"` default and fell through to the steps arm for anything else. A fixture whose steps moved under a different key replayed an empty list: zero ops, zero failures, **green** — with both ledgers still in agreement, since the fixture is named in `REPLAYED` and absent from `KNOWN_UNCOVERED`.

This is the honest version of why it survived the earlier sweep: gd's discovery is by explicit name, so *adding* a fixture cannot reach it, and the audit brief's SAFE/SPLIT test passes. The hole opens on an upstream **edit** to any of the 19 fixtures this binding claims. Two ledgers agreeing about a run that proved nothing is the same vacuous green the manifest guard exists to prevent — it just arrives by a different door.

Now an unmodelled shape fails naming the shape and both ledgers, a missing `shape` is drift rather than a default (all 21 canonical fixtures declare one), and `shape=steps` with no steps gets the twin of the existing scenarios rung.

Mutation-proven rather than asserted: with the dispatch reverted to HEAD, the new test fails with `an unmodelled shape replayed as 'steps' instead of failing: []` — and that empty failure array **is** the silent green, printed. `make check` OK, 27 test cases. `lazily-gd@fccdd12`.

---

Two follow-ups filed. The zig one matters for the item queued two heads down: `ingress_family_conformance.zig` still hardcodes its corpus path, so `#lzperturbaudit` would silently read the canonical corpus there while believing it had redirected — the exact failure `#lzzigspecdiroption` closed for the other zig runners last cycle. Filed before `#lzperturbaudit` in the queue for that reason.

Superproject pointers committed and pushed. CI is in flight on both repos (`cpp 31465829128`, `gd 31465830097`) — recorded pending, not waited on; the verdict here rests on local `make check` plus the probe table above. Exchange is at 369 lines (session-accretion `warn`); queue is active so I am not compacting.
<!-- agent:boundary:91e66a40:lazily -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-commit-push use subagents for fan-out tasks" priority go -->
- do [#lzjsscenarioswallow]
- ~~📌 do [#lzledgeragreementaudit]~~
- 📌 do [#lzzigdiffmutant]
- do [#lzzigingressspecdir]
- do [#lzperturbaudit]
- do [#lzspecschemasoverride]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#lzzigdiffmutant] lazily-zig's lossless-tree replay does not pin the anti-entropy FILTER, only convergence: mutating LosslessTreeCrdt.diff to ignore the partner frontier (ship every op in the log) reddens NONE of the nine fixtures, including non_contiguous_anti_entropy.json whose subject is a replica with a gap. deliver.only indexes into the diff, so a diff whose contents change should select different ops and change what the gapped replica renders (hole_before_repair expects b='013') — it does not, and the reason is not yet established. Either the runner's deliver path is not exercising the frontier as intended, or applyUpdate's idempotence absorbs the difference; find out which, then make the fixture discriminating (or record honestly that convergence is all it can pin). Contrast the at_byte char-offset mutant, which reddens six of the nine.
- [ ] [#lzzigingressspecdir] lazily-zig's ingress_family_conformance.zig:82 still hardcodes `const SPEC_DIR = "../lazily-spec/conformance/ingress/"` instead of routing through conformance_manifest.conformanceRoot(), so LAZILY_SPEC_CONFORMANCE_DIR does not move that runner. #lzzigspecdiroption fixed conformance_json.zig and reactive_map.zig but this seam was missed. Consequence for #lzperturbaudit: a perturbation probe that points zig at a scratch corpus gets a redirected reactive-graph/collections replay and a NON-redirected ingress replay in the same run, so an ingress fixture perturbed in the scratch copy cannot redden and the cell reads as convergence-only for the wrong reason. Route it through conformanceRoot(), then prove it the way #lzzigspecdiroption was proven: override to a byte-identical copy (green), to a copy with one perturbed ingress expectation (red), and to a nonexistent directory (panics, 0 skips). Audit the remaining zig runners for the same hardcoded-root split while there.
- [ ] [#lzperturbaudit] The corpus-perturbation audit that #lzoverrideallrunners existed to unblock has NOT been run. All five bindings (cs/go/dart/js/py) can now be pointed at a scratch corpus, but each has been probed with exactly ONE fixture, the one proving its seam. lazily-rs's AGENTS.md documents this audit as the pass a static detector cannot replace. For each of the five, walk the conformance families and perturb at least one fixture per family, recording which runners redden and which are convergence-only (a fixture that cannot be made to fail by flipping a declared expectation is pinning nothing). Expect finds of the #lzzigdiffmutant shape: an assertion that survives a mutation because an earlier assertion fires first, or because the runner never reaches it. Definition of done: a per-binding table of family -> perturbed key -> reddened runner, with every non-reddening cell explained rather than dropped.
- [ ] [#lzspecschemasoverride] LAZILY_SPEC_CONFORMANCE_DIR redirects the conformance CORPUS only; lazily-spec/schemas always resolves to the canonical sibling in go (specSchemasDir, 2 sites in ipc_conformance_test.go), js (schemasRoot/schemaPath, used by schema-conformance + lossless-tree-crdt + ipc) and cs (5 schema roots derived from SpecCorpus.Root via '..'). This is deliberate today because a corpus scratch copy carries no schemas/ sibling, but it means a probe that needs to perturb a SCHEMA has nowhere to point except the shared ../lazily-spec checkout, which reddens all nine bindings at once. That is the exact defect #lzoverrideallrunners just fixed, one level up. Add a spec-ROOT override (or a second LAZILY_SPEC_SCHEMAS_DIR) threaded through the same seams, default unchanged, fail-closed on an unreadable explicit value. Audit dart/py/kt/rs/cpp/zig/gd for the same split while there. Definition of done: flip one field in a scratch schemas copy and prove a schema-validating runner reddens without touching the canonical checkout.
- [ ] [#lzjsscenarioswallow] lazily-js scripts/check-scenario-coverage.mjs:202 swallows an unparseable corpus fixture with a bare `catch { continue; }` — the only unnamed swallow in that ladder. It cannot hide a coverage gap today (rung 1 still demands the file be opened, and any runner that opens it dies on JSON.parse), but a fixture that fails to parse is dropped from scenario accounting without naming itself, which is the shape every other rung in this repo refuses. Name the fixture and fail, matching the sibling rungs. While there: test/spec-corpus.cjs:106-113 exports listFixtures(), an enumerate-the-corpus helper with ZERO call sites — it is the exact primitive that would introduce the enumerating-host shape #lzledgeragreementaudit audits for, so delete it or pin it with a comment saying why it must stay unused.
- [ ] [#lzgdvocab] lazily-gd's reactive-graph runner is missing the fanout/churn/dispose_fanout/dispose_stale_handle bulk ops AND real id recycling, so 2 canonical fixtures remain: churn_returns_to_baseline.json and recycled_id_inherits_nothing.json. Recycling is the hard half and is a DESIGN decision, not runner vocabulary: Godot instance ids are monotonic and never alias, so 'a recycled id inherits nothing' has no natural referent — the binding needs an explicit id layer (a free-list over slot indices with a generation tag, the shape lazily-rs's SlotId uses) before the fixture means anything. Removing both entries from KNOWN_UNCOVERED in scripts/check-conformance-coverage.sh is the definition of done. DONE already: the merge_cell group (5 fixtures) landed with LazilyMergePolicy + Source.merge + ctx.source_with, and GDScript is now ~ on merge-algebra.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
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
