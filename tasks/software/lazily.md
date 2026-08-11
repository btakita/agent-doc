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

### Re: #lzzigdiffmutant — the premise does not reproduce; the real gap is one step sideways — opus-5

> **Queue prompt:**
>
> 📌 do [#lzzigdiffmutant]


**The fixture pins the filter. It always did.** Two independent mutations of the anti-entropy filter redden `non_contiguous_anti_entropy`:

| mutation | result |
|---|---|
| `diff` ignores the partner frontier (ship every logged op) | **red** — b renders `01`, fixture expects `013` |
| `DotRange.contains` degraded to per-peer-max version-vector semantics | **red** |

The second is the one that settles it. That is the *exact* failure the fixture's own description names — "a per-peer-max version vector would record the highest delivered dot and wrongly believe the hole was already filled, so it could never converge here" — and the corpus catches it. The fixture is not convergence-only.

I also chased the obvious explanation for the original observation and it does not hold. `#lzzigspecdiroption` had just closed a skip trap in this binding, so "the test never ran" was the natural suspect — but running the same ignore-the-frontier mutation on `10fef17`, the commit *before* that fix, reddens exactly the same two tests. Whatever produced the earlier "reddens NONE" reading, it was not the skip trap and it is not reproducible at either commit.

**What is genuinely unpinned is the thing that made the suspicion worth chasing.** `deliver.only: [0, 2]` indexes **positionally** into whatever `diff` returns. So I mutated the order rather than the contents:

| mutation | result |
|---|---|
| `std.mem.sort(...)` → `std.mem.reverse(...)` | **green — whole suite** |
| delete the sort entirely (log-order passthrough) | **green — whole suite** |

Both survived because the two indices select the same *set* either way, and rendering is order-independent — `applyUpdate` is order-tolerant by design, buffering an op whose parent has not arrived. So the fixture pins *which ops* cross the wire and says nothing about the order they arrive in.

That order is not a local implementation detail. It is a **cross-binding contract**, because `only` is positional: the canonical fixture means the same thing in every binding only while every binding returns the same order. lazily-rs (`sort_by_key(|op| (op.id.counter, op.id.peer))`), lazily-py (`sorted(...)`) and lazily-js (`.sort(cmpId)`) all produce it — so it is a real convention, held by agreement and pinned by nobody. A binding that dropped the sort as a redundant step would keep replaying the corpus green while `only` silently selected different ops than the fixture names.

The new test asserts it, and is built so it cannot pass by accident: a remote op with a lower counter than this replica's latest lands **last** in the log, so log order and canonical order differ — and the test asserts that difference explicitly before checking the sort. Without that, the assertion would hold for a reversed diff too and pin nothing. Both ordering mutants now redden it, and only it: 1074/1075.

Worth stating plainly, since the item was filed as a suspected vacuous fixture: **the fixture was doing its job; the gap was in the runner's addressing scheme.** `deliver.only` buys precise partial-delivery control at the cost of a hidden dependency on diff order, and that dependency had no test anywhere in the family. The contrast the item drew with the `at_byte` mutant (six of nine red) pointed at the right kind of question and the wrong subject.

`make check` OK. `lazily-zig@e8a2a28`, superproject pointer committed and pushed. Filed the follow-up for the other eight bindings, which have the same unpinned order.

Last cycle's CI landed green: `lazily-cpp` ✅ (3m42s) and `lazily-gd` ✅ (28s) both passed on the `#lzledgeragreementaudit` commits. zig's run for this commit is queued — recorded, not waited on. Exchange is at 435 lines (session-accretion `warn`); queue is active so I am not compacting.

### Re: #lzzigingressspecdir — one site turned out to be fourteen, and the override was half-working in silence — opus-5

> **Queue prompt:**
>
> 📌 do [#lzzigingressspecdir]


The item named `ingress_family_conformance.zig:82`. Auditing the rest, as it asked, found **fourteen sites across twelve areas**: nine runners each carrying their own `const SPEC_DIR = "../lazily-spec/conformance/<area>"`, plus ingress, `receipt` (×2), `ipc` (×3), `crdt_plane`, and `dependency_availability`. `#lzzigspecdiroption` moved `conformance_json.zig` and `reactive_map.zig`; everything else kept comptime-concatenating the default.

**So the override was half-working, and half-working is worse here than not working.** A run that reads the default corpus while believing it was redirected is green either way. There is no signal — not a warning, not a skip, not a differing path in the output. The binding advertised `LAZILY_SPEC_CONFORMANCE_DIR` support and delivered it for two runners out of fourteen.

I measured it rather than reasoning about it. Truncate fourteen fixtures across twelve areas in a scratch corpus, point the suite at it with the override:

| | tests reddened |
|---|---|
| before this change | **0** — `make test` exit 0, fully green |
| after | **26**, covering all twelve areas |

Zero. The same perturbed corpus that now reddens collections, coordination, crdt_plane, dependency-availability, ingress, ipc, membership, presence, rateshape, receipts, resilience, service, temporal and windowing produced a clean green run before.

That number is the whole point, and it lands directly on the item two heads down. `#lzperturbaudit`'s deliverable is a table of *family → perturbed key → reddened runner*, with every non-reddening cell explained. Run against the previous commit, **twelve of those areas would have filled in as "perturbation did not redden" — and the explanation would have been wrong in the most expensive way**: it reads as "this fixture pins nothing", a finding about the corpus, when the truth is that the runner never opened the perturbed bytes at all. That is exactly the false negative `#lzzigdiffmutant` turned out to be a report of, one cycle ago, from a different cause.

Four states, the same proof `#lzzigspecdiroption` used:

| | result |
|---|---|
| default, no override | green |
| override → byte-identical copy | green |
| override → perturbed copy | **26 red across 12 areas** |
| override → nonexistent directory | **panics, 0 skips** |

The third is the one that matters: it proves the override changes which *bytes* are read, not merely which path gets printed.

`specPath` / `specAreaPath` / `specAreaDir` in `conformance_manifest.zig` are now the only way to build a corpus path, and the `SPEC_DIR` constants are gone — an area is named as a bare subdirectory, so there is nothing left to concatenate a root onto. Fail-closed behaviour is unchanged: these reads already went through `specReadFile`, which panics on an explicit root that is not there.

**And a guard, because the fifteenth site would arrive just as quietly.** It scans `src/**.zig` for the default root and fails naming each offender; `conformance_manifest.zig` is the one legitimate mention. Line and doc comments are skipped deliberately — several runners quote the old path while explaining the corpus, and making them stop describing it would trade a real explanation for a lint. Proven both directions: reintroducing a `const SPEC_DIR` in `service.zig` fails naming that file, and pointing the scan at an empty tree fails as a pass over nothing rather than reporting OK, the same positive-evidence rule the fixture and scenario rungs already carry.

It lives in `check-conformance-coverage.sh` rather than a zig test because this toolchain's `std.fs` has no `cwd` — the same gutting this module's own header documents — and there it is already inside `make check` and reached by CI. lazily-go carries the equivalent guard as an AST test, which it can afford in-language.

`make check` OK, 147/147 scenarios. `lazily-zig@07b1605` + `ec8e65e`, superproject pointer pushed. Filed the follow-up: `#lzoverrideallrunners` declared five other bindings redirectable after probing each with **one** fixture, and one fixture is precisely what would have passed here while twelve areas stayed pinned to the default.

Last cycle's CI: `lazily-zig` ✅ green on the `#lzzigdiffmutant` commit. The two runs for this change are in flight. Exchange is at 472 lines (session-accretion `warn`); queue is active so I am not compacting.

### Re: #lzoverrideallrunnersaudit — measured all nine; the reference binding was the worst of them — opus-5

> **Queue prompt:**
>
> do [#lzoverrideallrunnersaudit]


Measured rather than inspected, one binding per agent, each with a byte-identical-copy control so the env var alone could not be mistaken for a signal.

| binding | areas reddened | verdict |
|---|---|---|
| lazily-cs | 25 / 25 | REDIRECTABLE |
| lazily-go | 25 / 25 | REDIRECTABLE |
| lazily-js | 26 / 26 | REDIRECTABLE |
| lazily-py | 24 / 24 | REDIRECTABLE |
| lazily-dart | 24 / 24 | REDIRECTABLE |
| lazily-cpp | 24 / 24 | REDIRECTABLE |
| lazily-gd | 1 / 1 family | REDIRECTABLE |
| lazily-kt | 26 / 26 — **but only via `LAZILY_SPEC_DIR`** | **broken for the documented variable** |
| **lazily-rs** | **0 / 25** | **NOT REDIRECTABLE** |

**The original five were fine.** `#lzoverrideallrunners`' claim about cs/go/dart/js/py holds up under a proper per-area probe — the one-fixture spot check happened to be right. I expected to falsify that and did not.

**lazily-rs was worse than zig, and it is the reference binding.** Every conformance test declares its own `const SPEC_DIR: &str = "../lazily-spec/conformance/<area>"` — 47 sites across 41 files — and no runner read an environment variable at all. Only the coverage guard did. So the guard moved and not one replay did: **zero of twenty-five areas reddened**, while the same bytes read directly redden all twenty-five. The fixtures were fine; the runners never opened them.

That is the expensive version of this defect. `#lzperturbaudit` names lazily-rs's own AGENTS.md as the document arguing that a perturbation sweep is the pass a static detector cannot replace — and run against rs before this change, that sweep would have produced twenty-five cells reading "this fixture cannot be made to fail", every one of them a statement about a file nothing opened.

**lazily-kt was broken in a way a variable-name convention hides.** `ConformanceFixtures` read only `LAZILY_SPEC_DIR`, while its own coverage guard has read `LAZILY_SPEC_CONFORMANCE_DIR` first all along. Point the documented variable — the one every sibling honours and the one this audit uses — at a scratch corpus with a truncated fixture: **447 tests passed, exit 0.** The guard's output changes and the replays do not, so the run looks redirected and is not.

---

Both are fixed, and both fixes are measured, not argued.

**rs** (`c6c50b7`): the redirect lives at `spec_read_to_string`, the single seam every corpus read in `tests/` already passes through, rather than being threaded into 47 constants. That is deliberate — rewriting the constants fixes the sites someone remembered; this cannot miss one, and it covers every future runner that uses the seam, which the seam's own doc comment already requires. `conformance_id` had the same defect a layer down: it derived manifest ids by finding the literal `lazily-spec/conformance/`, so a scratch corpus not named `lazily-spec` recorded nothing and the guard failed with *missing evidence* for a run that had replayed everything. After: **14 targets across 14 areas red** under plain `cargo test --tests` (the rest are feature-gated in that invocation), with the control green at `144/150 fixtures OPENED` — the same count as the default corpus, which is what proves ids resolve against the scratch root instead of going empty.

**kt** (`ba7f41d`): `LAZILY_SPEC_CONFORMANCE_DIR` now wins over `LAZILY_SPEC_DIR`, both are declared as the Gradle corpus input and forwarded to the test JVM (the fingerprint and the replays must agree on which corpus is in play, or Gradle reuses a result computed against another one), and the schema read moves off `root.resolveSibling(...)`, which is wrong once `root` can be an arbitrary directory. After: the same run fails `PresenceConformanceTest > awareness()` — the perturbed fixture's own test, exact attribution. Also removed `StateChartConformanceTest.specDir`, a dead field spelling the default root verbatim that sat through every green run: the standing proof that nothing there guards against re-spelling it.

CI green on both, and on the two zig commits from last cycle.

**One correction on my own process:** I pushed the kt fix before reading that `make check` had failed `spotlessKotlinCheck`. Fixed in `c55eb8d` and `make check` is green, but the commit went out red and I should have read the exit code before pushing.

---

**What the audit found that the item did not ask for.** Seven of nine bindings have **no** guard against a source spelling the default corpus root — and the two that do were both proven evadable by synthetic violation: go's scans only root-package `_test.go` files and misses a joined `"../lazily-spec"` prefix; js's misses joined strings and walks `test/` only. So the property currently holding in eight bindings is convention, not enforcement, in every one of them. Filed. Also filed: rs's 47 constants and its presence probes still name the default root, and a cluster of corpus-absence inconsistencies (cpp fails 6 suites that should SKIP, kt's `.optional(true)` does not tolerate a nonexistent dir as its comment claims, cs's guard emits a raw Python traceback on an unparseable fixture, and js's hand-maintained `AREAS` list has drifted to omit 8 areas it replays).

`#lzperturbaudit` is now unblocked for all nine — and its cells are interpretable, which before this they were not for rs. Exchange is at 515 lines (session-accretion `warn`); queue is active so I am not compacting.

### Re: #lzcorpusrootguards — all nine guarded, and the evasion I warned them about was real in every one — opus-5

> **Queue prompt:**
>
> do [#lzcorpusrootguards]


| binding | guard | commit |
|---|---|---|
| cs | new rung, C# string-literal lexer | `c91d3dc` |
| py | new AST meta-test | `96c76e9` |
| dart | new Dart meta-test | `aba0edd` |
| gd | new rung, GDScript folder | `8ee88b5` |
| go | **hardened** (AST, 3 holes closed) | `d605e46` |
| rs | new rung, enumerated baseline | `b5efc9b` |
| kt | new rung, Kotlin lexer | `8e0f8ec` |
| cpp | new rung, C++/CMake tokenizer | `511d572` |
| js | **hardened** (tokenizer, 2 holes closed) + AREAS derived | `5e4b766` |

All ten CI green (zig landed `ec8e65e` last cycle). Every guard proved four things by running them, not by asserting them: passes clean; fails on a single-literal violation naming the file; fails on the joined-segment form; and **fails when the scan examines nothing**, because a walk that reaches zero files and reports OK is the same vacuous green the fixture and scenario rungs already refuse.

**The joined-segment hole was not a go/js quirk — it is what this shape of guard does by default.** lazily-dart's agent wrote its guard, then proved its *own first draft* evadable by `p.join('..', 'laz' 'ily' '-sp' 'ec', 'conformance')`, and hardened it. That is the third independent implementation to fail the same way before being fixed. Every guard here now folds concatenation before matching rather than grepping for a needle, and several are real lexers because comments have to stay legal — roughly twenty runners per binding legitimately quote the corpus path while explaining what they replay, and a guard that outlawed that would trade a real explanation for a lint.

**rs needed a different shape and got one.** It still spells the default root in 45 references across 41 files; those are not a correctness bug since `c6c50b7` redirects at the seam, so a guard failing on all of them would be unlandable — and one that skipped files containing the literal would be a guard that passes by looking away. So the baseline is enumerated and checked in **both** directions, like this family's `KNOWN_UNCOVERED` ledgers: a new site fails, and a listed site that disappears fails as stale. The debt is now countable and cannot grow, and `#lzrsspecdirconsts` retires it.

**Two corrections from the agents, both of which I'd rather have than not.** js measured **25** replayed areas plus `egress` excused, not the 26 I put in the brief — the corpus root's 8 top-level fixtures are not an area. And cpp's allowlist entry for its seam header is a declaration of entitlement, not a necessity: that file does not currently spell the fragment at all, and the agent said so rather than implying the entry was load-bearing.

Two things worth carrying forward. cpp already had the strongest guard of the family and it is not a text scan at all — an `#error` when the corpus macro is undefined, so a runner that forgets to register cannot compile; the new rung exists only to cover translation units that never include the seam. And every agent independently flagged the same residual limit: a root assembled at runtime from computed pieces escapes any source-level scan. Each said so in its own comment instead of implying total reach, which is the right disposition — nobody writes that by accident, and doing it deliberately is no longer deniable as an oversight.

The js AREAS list is also fixed and no longer hand-kept: areas are now derived from the call sites the runners actually spell, with a reverse check that every corpus area is replayed or excused by name. It had drifted to 17 of 25 while its own comment warned that a missing entry lets an area go dark unnoticed.

Superproject pointers bumped in one commit. Exchange is at 562 lines (session-accretion `warn`); queue is active so I am not compacting.

### Re: #lzrsspecdirconsts — 45 constants retired, and the probes were reading a corpus the run wasn't replaying — opus-5

> **Queue prompt:**
>
> do [#lzrsspecdirconsts]


`c6c50b7` made the override reach the replays by redirecting at `spec_read_to_string`. That is complete for reads — but **a redirect at the read is silent about a path that is never read**, and that is exactly what a presence probe is.

Demonstrated rather than reasoned about. Point the override at a corpus copy with `rateshape/debounce.json` removed:

| | result |
|---|---|
| before | probe checks the **default**, finds `debounce.json` there, returns true, the test proceeds — and the redirected read panics: `failed to read fixture ../lazily-spec/conformance/rateshape/debounce.json` |
| after | probe checks the corpus in play, returns false, the suite skips |

The panic message is the tell: it names a path this run was not reading. The probe and the read were talking about two different corpora, and the only reason it surfaced at all is that I removed a file — with the file present, the probe answers about a directory nobody opened and its skip is indistinguishable from a pass.

**`common::SpecDir` carries the area alone and resolves the root on `Display`.** So `format!("{SPEC_DIR}/{name}")` keeps working verbatim while producing a path under whichever corpus is in play, and it stays a `const` — no call site changed shape. That mattered: there are ~70 of them using inline format args, and rewriting those by hand is precisely where a mechanical pass goes wrong. `path()`, `join()`, `exists()` and `is_dir()` cover the ~15 sites that wanted a `Path` instead of a string, and those turned out to be exactly the probes and directory reads that were pointed at the wrong corpus. The compiler found every one of them.

**The guard's baseline is now empty, and it earned that.** With the conversion in place it reported all 44 entries **STALE** before I touched the list — the both-directions half of that ledger doing its job rather than an edit nobody checked. Zero default-root spellings remain under `tests/` outside the seam and prose.

**The measurement is unchanged, which is the point.** The same perturbed corpus reddens the same 14 targets it did after `c6c50b7`, and the byte-identical control stays green. This retires debt without moving coverage — if the count had shifted either way I would have introduced something.

`make check` OK. `lazily-rs@44e32da`, superproject pointer pushed, CI running.

**Something I should have been checking all session:** this superproject's own `Agent Doc` workflow has failed on every pointer bump I have pushed — and on runs going back well before this session. It is not a regression: every job reports *"The job was not started because recent account payments have failed or your spending limit needs to be increased."* That is GitHub billing on the `agent-loop` repo, an external CI-start blocker with no logs, and it means **the superproject has had no CI coverage for some time**. The binding repos are in a different org and have been running and passing throughout, so every verdict I have reported this session still stands on their CI plus local `make check`. But I was reading binding CI and not the superproject's, and I should have noticed sooner.

Exchange is at 595 lines (session-accretion `warn`); queue is active so I am not compacting.

### Re: #lzperturbaudit — the audit ran, and four bindings have fixtures that pin nothing — opus-5

> **Queue prompt:**
>
> 📌 do [#lzperturbaudit]


The probe this item needed is not the one I ran earlier. Truncating a fixture proves the runner **read** it; flipping a declared expectation to a wrong-but-valid value proves the runner **asserts** on it. Every binding here is confirmed redirectable first, so a green cell is interpretable — it means the fixture pins nothing, not that the runner ignored the bytes.

| binding | areas reddened | keys probed | verdict |
|---|---|---|---|
| cs | **25 / 25** | 41 | 5 green cells, 1 real defect |
| go | **25 / 25** | 57 | 7 green cells, 2 real defects |
| py | **24 / 24** | 74 | 15 green cells, 2 structural defects |
| js | **26 / 26** | 38 | 0 undeclared green cells |
| dart | **22 / 24** | 35 | 2 fixtures pinning nothing |

Every area asserts its headline expectation in every binding — these suites are not reading fixtures and discarding them. The findings are all one rung down, and there are five real ones.

**dart: `crdt-tree/algebra.json` pins nothing at all.** The runner consumes `seed`, `replicas`, `merge_orders` and the ids, and every assertion is self-referential: merge order *n* is compared against merge order 0, the restored replica against its own source, and `expect.delta` / `expect.apply_changed` against Dart literals rather than the fixture. All four declared expectations flip clean — **and so does the scenario's seed text.** The runner will happily replay `"WRONG\n"` and pass, because it only ever checks that three merge orders agree with each other. The only thing keeping that file honest in this binding is the mirror-drift check, which compares bytes, not behaviour.

**dart: every per-frame `assertions` block in `signaling/frames.json` is dead.** The runner reads `wire`, `direction`, `label` and asserts round-trip identity; `peer`, `to`, `from`, `capabilities` are never passed to the tracker. Set them to nonsense and the suite is green — not even a drift hit, since `signaling/` is not mirrored.

**go: `stable_keys_not_invalidated` is asserted against a no-op.** The runner primes readers, then reconciles to the target order — but the map was *already* reconciled to exactly that target one line earlier. The sibling reorder the fixture's own comment names never happens, so nothing invalidates and every reader is trivially warm. `["b","c","a"]` passes, `["b"]` passes, **`["zzz"]` — a key not in the map — passes.** Only `[]` reddens, via an emptiness guard. The fixture's central claim is untested.

**go: no `assertions`-block integer above 2^53 can be pinned.** `assertKey` compares by marshalling the actual value to JSON and back into `any`, which lands in `float64`. At `arena_blob.json`'s checksum (~1.3e19) the double spacing is 2048, so ±1 is invisible; +7,700 reddens. Only one cell is affected today and its typed `expected` twin still catches the error — but it is a live trap for any future fixture putting a hash or u64 id in an `assertions` block as a bare number.

**py: the stdlib anti-vacuity budget is unread.** `scenario_floor`, `assertion_floor` and `mutation_floor` are *required* by the stdlib fixture schema and have **zero** references anywhere in the binding — `scenario_floor: 99` against a six-scenario fixture is green. The mutation ledger beside them is self-referential: it checks `must_fail` ids are a subset of the fixture's own scenario ids and never applies a mutation, so `mutations: []` passes and rebinding an operator to an unrelated scenario passes. The one area whose fixtures carry an explicit "prove you did N things" contract is the area where nothing checks it.

**py, and go: list-shaped expectations checked by per-element membership never assert their own length.** `distributed.expect.converged` and `lossless-tree.expect.converged` both loop the declared wants and search for each — so **deleting an entry silently removes an obligation**. The corpus can shrink its own expectation and stay green. In lossless-tree it degenerates further: the check compares `names[1:]` against `names[0]`, so a one-element list runs zero comparisons.

**The structural finding, which is why several of these survived the existing guards.** Every binding has an unconsumed-key guard, and each is blind in the same way: dart's tracker only sees blocks a runner explicitly *binds*, so a block nobody passes in cannot be "unconsumed"; go's `assertKeyWith` marks a key asserted when the callback *returns*, not when it compares anything, so a callback that ignores its argument satisfies the guard; cs's has the same property, which is how an `Assert.All` over an empty array counts as an assertion. **The guards prove keys inside reached blocks were read. They say nothing about blocks no runner ever reaches, or callbacks that read without comparing.**

Two honourable mentions. **js is the cleanest** — 26/26, and its only non-reddening cells are six parked reactive-graph fixtures that are declared in a skip ledger asserted in both directions *and* in the assertion-key guard's `DECLARED_UNCONSUMED` list with a stale-entry check. Pinning nothing today, by declaration, with a tripwire on the declaration. And **cs's prose convention is load-bearing in the right direction**: flipping a prose paragraph is green, but emptying the `prose: ["note"]` declaration reddens, because `note` then becomes an unconsumed key.

Five follow-ups filed, one per defect. Exchange is at 625 lines (session-accretion `warn`); queue is active so I am not compacting.

### Re: #lzdartvacuousfixtures — 16 vacuous cells, all now reddening — opus-5

> **Queue prompt:**
>
> do [#lzdartvacuousfixtures]


Both fixtures are bound. Every declared expectation that could previously be flipped to nonsense without consequence now fails a replay runner.

| fixture | cells | before | after |
|---|---|---|---|
| `crdt-tree/algebra.json` | 7 declared expectations | 0 redden | **7 redden** |
| `signaling/frames.json` | 9 per-frame assertion keys | 0 redden | **9 redden** |

**crdt-tree** was asserting against itself. Merge order *n* was compared to merge order 0, the restored replica to its own source, and `expect.delta` / `expect.apply_changed` to Dart literals — so the file could have declared `texts_equal: false` and passed. The properties are now accumulated and handed to `assertKey`, which is the difference that matters: the **fixture's claim** is what fails, rather than a bare `expect` that was true no matter what the file said. `delta` compares the wire form of the emitted ops rather than just asserting emptiness, so `delta: [{...}]` fails too.

**signaling** is the more interesting one, because it explains why nothing caught it. The runner read `wire`, `direction`, `label` and asserted round-trip identity; the `assertions` block on all 17 frames was never passed to `assertionsOf`. **A block no runner binds cannot be reported "unconsumed" — the guard only sees blocks handed to it.** That is the `#lzunboundblockguard` shape in the concrete, and it is why an audit that flips values found this while a guard designed to find unread keys did not.

All nine keys are now read off the **decoded** message re-serialized, not off the fixture's input map. That distinction is load-bearing: asserting against the input would have been satisfied by echoing it back, whereas a decoder that dropped `peer` now fails. Three of the nine are derived rather than copied — `has_capabilities`, `roster_excludes_self` (a welcome roster never names the recipient back to itself) and `server_stamped_from` — so they pin a property rather than a field.

One thing I deliberately did **not** do: the audit noted that corrupting `algebra.json`'s seed text also passes. That is correct behaviour, not a gap — the seed is an input, and the scenario's claim is order-independence, which holds for any seed. Making the seed pinnable would mean inventing an expected text the corpus does not declare.

`make check` OK, control on a byte-identical copy green. `lazily-dart@d5ccebd`, superproject pointer pushed, CI queued.

Exchange is at 662 lines (session-accretion `warn`); queue is active so I am not compacting.

### Re: #lzgoassertkeygaps — an assertion against a no-op, and a comparison that could not resolve its own inputs — opus-5

> **Queue prompt:**
>
> do [#lzgoassertkeygaps]


**`stable_keys_not_invalidated` was asserted against a no-op.** The runner primed its readers *after* the prior→target reconcile, then ran a second reconcile to the same target. That invalidates nothing by construction, so every reader was trivially warm.

| declared list | old | new |
|---|---|---|
| `["b","c","a"]` (over-claim) | green | **red** |
| `["b"]` (shrunk) | green | **red** |
| `["zzz"]` (key not in the map) | green | **red** |
| `[]` | red | red |
| `["c","b"]` (permutation) | green | green — correct, it is a set |

Readers now prime on the **prior** state, before the real reorder. That alone is still a lower bound — a shortened list just stops being checked — so the declared set is also compared against `prior ∩ target` minus the keys named by a `move` op. That is the spec's own definition of an LIS member, **derived from data the fixture already declares** rather than restated in the test, which is what makes `["b"]` a failure instead of a smaller obligation.

**`assertKey` could not resolve any integer above 2^53.** Both sides of `jsonValueEqual` are `any` decoded from JSON, so both are `float64`; at `arena_blob.json`'s 1.3e19 checksum the spacing between representable doubles is 2048, so ±1 is invisible while +7700 — past one ULP — reddens.

The important thing here is what *couldn't* be fixed. The precision is gone before the comparison starts: the fixture was decoded into `float64` long before `jsonValueEqual` sees it, so nothing at that layer can recover the digits. Decoding the whole corpus with `UseNumber` would fix it properly but rewires ~30 runners' decode paths, which is out of proportion to one cell. So the comparator now does the one honest thing available — **a comparison that cannot resolve its own inputs fails and says so**, naming the two exact channels (a typed `expected` field, or a decimal string, which the corpus already uses for `codec/nodeid_exact_range.json`).

That guard fires on the one cell affected today, so the redundant `assertions` copy of the checksum is excused with its reason. Redundant, not unchecked: `expected.descriptor.checksum` is a typed `uint64` compared with `==`, and I verified ±1 there reddens.

Worth naming the shared shape, since it is the third instance this session: **an assertion satisfied by its own bookkeeping rather than by the system's behaviour.** The no-op reconcile satisfied `assertKeyWith` because the callback returned; the float comparison satisfied it because the numbers matched *after* both were degraded. Neither guard was wrong about what it measured — they were measuring the wrong thing.

`make check` OK. `lazily-go@c5cce05`, superproject pointer pushed. Last cycle's `lazily-dart` CI came back green.

Exchange is at 688 lines (session-accretion `warn`); queue is active so I am not compacting.

### Re: #lzpystdlibfloorsunread — the anti-vacuity budget nothing was reading — opus-5

> **Queue prompt:**
>
> do [#lzpystdlibfloorsunread]


Three floors — `scenario_floor`, `assertion_floor`, `mutation_floor` — are **required** by `schemas/stdlib-fixture.schema.json`, and lazily-py referenced none of them. `scenario_floor: 99` against a six-scenario fixture was green. The one conformance area whose fixtures carry an explicit *"prove you did N things"* contract was the one area where nothing checked it.

| probe | before | after |
|---|---|---|
| `scenario_floor` 6 → 99 (timer) | green | **red** |
| `assertion_floor` 15 → 9999 (timer) | green | **red** |
| `mutation_floor` 3 → 999 (timer) | green | **red** |
| `mutations: []` | green | **red** |
| `scenario_floor` 8 → 99 (timeout) | green | **red** |
| `assertion_floor` 70 → 9999 (revision_barrier) | green | **red** |

Each floor compares against what the **run** did, never the file: `replayed` counts scenarios the ledger actually yielded, and the assertion count increments inside `_assert_expect`, at the comparison itself. A floor computed from the fixture would be a tautology — which is precisely the failure being fixed.

**Writing that counter reproduced the same shape one level down, which is the part worth keeping.** My first version used `isinstance(expect, dict)` — and that is **false** here, because `scenarios()` hands out tracked views rather than plain dicts. So it counted 1 per step instead of per key: 14 for `timer.json` where the real figure is 29. The floor is 15, so it failed loudly and I found it. Had the floor been 12, it would have passed — a check measuring the wrong thing, satisfied by an accident of type. That is the third variant of this session's recurring shape, and I only caught it because the corpus had set its floor high enough to notice.

**One thing I did not fix, stated plainly rather than implied.** This binding still does **not** apply the mutations. It checks the ledger is well-formed, references real scenario ids, and now meets its floor — but it never mutates anything, so rebinding an operator to an unrelated scenario stays green. lazily-rs does this properly: it replays each operator through an *independent interpreter* and asserts the named scenarios fail. Porting that is real work with a real design (you need a second implementation to mutate, or you are perturbing the code under test), so it is filed as `#lzpystdlibmutants` rather than half-done here. `mutation_floor` currently bounds the ledger's size and nothing more, and the code says so.

Also worth noting: the coined-id hook blocked my commit because the message referenced `#lzpystdlibmutants` before the item existed. Correct catch — I filed it, then committed.

`make check` OK, 781 passed. `lazily-py@c0cb635`, pointer pushed. `lazily-go` CI from last cycle came back green.

Exchange is at 719 lines (session-accretion `warn`); queue is active so I am not compacting.

### Re: #lzconvergedlistlength — four lists that could shrink without failing — opus-5

> **Queue prompt:**
>
> do [#lzconvergedlistlength]


A per-element membership check only ever *adds* obligations. It never counts them, so a declared list can lose an entry — or name something that does not exist — and the loop simply has less to do.

| binding | expectation | before | after |
|---|---|---|---|
| py | `distributed.expect.converged` drop an entry / `[]` | green | **red** |
| py | `lossless-tree.expect.converged` `["a","b"]→["a"]` / `[]` | green | **red** |
| go | `rpc.unresolved_after_frame_indices` `[7]` / `[0]` / `[]` | green | **red** |
| cs | `receipts.assertions.nonterminal_outcomes` `[]` / drop / reorder | green | **red** |

Each fix asserts the declared list against **what the run produced**, not against itself:

- **py distributed** — the declared count must equal the number of entries the plane converged.
- **py lossless-tree** — the worst of the four. It compares `names[1:]` against `names[0]`, so a one-element list runs **zero comparisons**. The declared set must now cover every replica the scenario built, and carry at least two.
- **go** — the run records every frame the call really was pending after, and the declared list is compared against that set. `[7]` named no frame at all, so it asserted nothing.
- **cs** — `Assert.All(want, item => !ParseOutcome(item).IsTerminal())` was a property of the **enum**, not of the replay: the callback never touched the projection. `Assert.All` over an empty sequence is vacuously true. It now compares against the outcomes the projection actually recorded as non-terminal.

The cs one is worth dwelling on, because it explains why the existing guards were blind to all four: **the key was still marked SATISFIED.** An assertion that receives its argument and never compares it is indistinguishable, to a guard that only checks whether the callback ran, from one that does the work. That is `#lzunboundblockguard` — the next item — seen from the other side.

Permutations stay green in py and go, deliberately: converged replicas and pending frame indices are sets, so I compare sorted. In cs the receipts are a sequence, so order is pinned there. Both choices are stated in the code rather than left to inference.

dart needed nothing: the audit showed its `eager_present` and `nonterminal_outcomes` are order-blind but **do** redden on drop, which is the correct shape and a useful contrast.

`make check` green in all three. `py@e94dd61`, `go@53cf20d`, `cs@1df7e7e`, pointers pushed. py CI from last cycle came back green.

Exchange is at 749 lines (session-accretion `warn`); queue is active so I am not compacting.

### Re: #lzunboundblockguard — the guard against blind spots had one, in every binding — opus-5

> **Queue prompt:**
>
> do [#lzunboundblockguard]


**The headline is not the fix, it is the measurement.** Each binding already had a rung-0 "bind ledger" whose job is to catch assertion blocks no runner ever touches. Every one of them enumerated blocks by looking in a **fixed handful of places** — the root `assertions`, plus one hard-coded level inside `frames`/`scenarios`/`rejects` — and then reported its own sample as complete:

| binding | blocks it inventoried | blocks the corpus carries | after |
|---|---|---|---|
| cs | 33 | 690 | **690** |
| js | 32 | 583 | **596** |
| py | 31 | 578 | **578** |
| dart | (none — no ledger) | 672 | **647 bound, 25 excused** |
| go | (none — no ledger) | — | full walk added |

cs printed `33/33 bound, OK`. js was auditing a **5% sample**. A fixed-place inventory cannot report a block it never looked for, so the rung that exists to catch the null form was wearing it.

All five now walk every fixture the **runtime manifest** says was opened, at any depth, including through array elements — the shape dart's dead signaling blocks had. Each carries a both-directions escape hatch (an excuse for a block that IS bound fails as stale; one naming a path the fixture no longer carries fails as rotted) and a vacuity floor.

**Widening the walk found live defects in four of the five**, none of which any existing guard could see:

- **cs — 7 scenarios replayed against hand-written literals** while their own `expect` blocks were compared by nothing (`crdt-tree/algebra`, `reliable-sync/coalesce`, `liveness_lease_eviction`). Now bound and compared against what the replay produced.
- **dart — a dead assertion since 2026-07-01.** `collections_conformance_test.dart` built its readers from `map.keys()..clear()`; `keys()` returns a copy, so the cascade emptied it and the per-key loop asserting `invalidates.value` never ran. The library was already correct; the test had stopped checking.
- **js — `signaling/anti_spoof_session.json` carries `steps[].expect` as an ARRAY** (8 lists, 30 keys). The recorder instrumented only object-valued blocks, so every one sat outside every rung while two hand-written `deepEqual` lines audited by nothing did the work.
- **go — two holes:** `codec/frame_roundtrip_json.json`'s JSON half read `expect` with hand-picked keys while its msgpack sibling bound the same block properly, and `dependency_reactive_availability.json` was decoded with a plain `Unmarshal`, which drops unmodelled keys silently.

**Direction (b) — callbacks that receive a value and never compare it — came out unevenly, and the honest answer differs per binding.** go, py, dart and js built runtime witnesses (a recording view; a key whose callback never touched the value fails). **cs did not, and said why rather than half-building it:** its runners *record* divergences into a ledger instead of throwing, so the differential approach it spiked signalled nothing and produced 288 phantom "insensitive" keys. Closing it there means giving those runners a comparison seam the tracker owns — a runner rewrite, not a guard. I would rather have that written down than a mechanism that appears to work.

Three residues are documented in code rather than implied: scalars can't be proxied in dart; `str`/`bool`/`None` can't in py (19 of 130 sites); and primitives in js rest on lazily-spec's existing **static** gate, which parses `assertKeyWith` callbacks and rejects ones that never mention their parameter. The static and runtime checks are complementary — neither subsumes the other.

`make check` green and CI green on all five: `dart@034c6cf`, `go@7c49128`, `cs@167c8ca`, `py@3b8463d`, `js@2d0bfbd`. Two follow-ups filed.

Exchange is at 782 lines (session-accretion `warn`); queue is active so I am not compacting.

### Re: #lzspecschemasoverride — schemas redirect independently now, in all six bindings that validate against one — opus-5

> **Queue prompt:**
>
> 📌 do [#lzspecschemasoverride]


| binding | sites | commit |
|---|---|---|
| rs | 10 (2 files) | `2266baa` |
| py | 11 (3 modules) | `097662c` |
| cs | 5 | `7594757` |
| go | 3 + a second in-repo root | `aa0bd4e` |
| js | 2 | `c0a6ebd` |
| kt | 1 | `d8b6c08` |

dart, cpp, zig and gd resolve no schemas at all — nothing to do, and I checked rather than assumed (dart's only hit is a comment in a shell script).

Every one proves the same four states, and the third is the one that matters: **override → a copy with one field flipped → RED**, naming the failing test. That is what shows the override changes which *bytes* are validated against rather than which path gets printed. `snapshot.json`'s `epoch.type` `integer`→`string` reddens rs; `defs.json`'s `wall_time.type` reddens py; `ffi.json`'s enum reddens js. A nonexistent directory fails loudly in all six — never a skip, never a silent fall back to the canonical tree.

**cs had a second consequence that made the coupling actively harmful.** Its schemas path was `Root/../schemas`, so pointing `LAZILY_SPEC_CONFORMANCE_DIR` at a corpus-only scratch copy *broke the run* — 10 `DirectoryNotFoundException`s, which an earlier audit this session had to work around by copying sibling directories. Decoupling fixes that: the same invocation is now green, and the agent proved it by re-running the pre-fix code against the identical scratch corpus and getting the old 10 failures back.

**go found a root nothing could ever have reached.** Both schema consumers spelled an in-repo `filepath.Join("schemas")` *ahead* of the sibling — a second location no override, old or new, could redirect. Found by AST scan rather than grep.

**kt produced the finding worth carrying forward, and it is about the test harness, not the schemas.** With `inputs.dir` + `PathSensitivity.RELATIVE` alone, the byte-identical-copy proof came back `11 actionable tasks: 11 up-to-date` — Gradle fingerprinted the scratch copy identically to the default and **never started a JVM**. The row meant to prove the override took effect had proved nothing. It needed `inputs.property` as well. That means the corpus comment added earlier this session, claiming that forwarding the env var *"makes the variable part of the task's input fingerprint"*, is **false as written**: it holds only when the content differs. The agent fixed the schemas input and deliberately left the corpus input alone as out of scope; I have filed it.

Four of the six also widened their corpus-root source guard to cover the schemas root, on the reasoning that a respelled default schemas path opts back out of the new override exactly as a respelled corpus path opts out of the old one. Each proved it with an injected violation.

All six `make check` exit 0 and CI green; `../lazily-spec` was never written to by any agent — every perturbation lived in a scratch copy.

Exchange is at 818 lines and session-accretion has crossed to `block`; the queue is active, so per its own guidance I am continuing to drain rather than compacting, and noting the size here.

### Re: #lzcorpusabsencehandling — three comment/behaviour mismatches, and one of them was mine — opus-5

> **Queue prompt:**
>
> do [#lzcorpusabsencehandling]


All three fixed, all CI green: `cpp@d052c4f`, `kt@22e75e0`, `cs@35b9e08`.

**cpp — fixed structurally rather than by a fourth list edit.** Six suites exited 77 to mean SKIP while missing from the `SKIP_RETURN_CODE 77` registration, so CTest read them as failures; the comments beside that list recorded having fixed the same omission twice before. A single `lazily_add_spec_conformance_test()` now states all three facts about a corpus-reading suite in one place, and a configure-time guard walks each target's transitive include closure and **fails configure** if anything reaching `require_spec_checkout_or_skip` was registered with a bare `add_test()`. Measured: 30 skips + 6 failures → **36 skips, 0 failures**, and the CI hard-error property still bites (both existing guards replayed red on an absent corpus).

**kt — I asked for either fix, and the agent chose the other one, correctly.** I leaned toward making a sibling-less checkout runnable. It verified three things first: `requireRoot()` *throws* rather than skipping, so the comment's premise was already false; `optional(true)` was inert (it excuses an absent provider *value*, not a nonexistent directory); and `proto { srcDir("../lazily-spec/proto") }` is hardcoded, so **a sibling-less checkout cannot compile at all** — 28 unresolved references before `:test` is even configured. There is no state where this repo builds without `lazily-spec`, so there was nothing for the input to be lenient about. Making configuration lenient would have traded one loud failure for ~450 red replays. It kept the hard failure and corrected the comment.

**cs — the exposure was in six places, not one.** Beyond the reported scenario-leg `JSONDecodeError`, it found and *reproduced* a `UnicodeDecodeError` traceback on an interleaved manifest and `ValueError` tracebacks on non-integer `MIN_BLOCKS`/`MIN_SCENARIOS`. All six now name the file and say what to do, all still hard failures, and the failure lands *before* any coverage arithmetic — a count over a partly-readable corpus is untrustworthy in both directions.

**And a correction to my own prior cycle.** The rationale I had an agent write into `d8b6c08` last cycle claims that a missing directory under an explicit override "fingerprints the same as no sibling at all (both absent, because the input is `optional`), so a probe pointed at a nonexistent tree could be answered from a cached GREEN." **That case cannot occur** — a missing input directory fails Gradle task validation outright, and `optional(true)` does not change it. The `inputs.property` is still justified by the *other* half of its argument (a byte-identical copy at another path really does fingerprint identically, measured), so nothing is broken — but half of that comment is folklore I introduced, and I have updated `#lzktgradleinputfingerprint` to say so and to require deleting it.

Two process notes. The coined-id hook blocked a commit because my message contained `#include` — same false-positive class as `#error` two cycles ago; a C preprocessor directive read as an untracked backlog id. And the kt agent hit the same hook writing `#lzvacuousrun` into a source comment, so it cited the class by name instead.

Exchange is at 850 lines, accretion `block`; queue is active so I keep draining per its guidance.

### Re: #lzdifforderallbindings — the order is pinned everywhere, and cpp was not following it — opus-5

> **Queue prompt:**
>
> do [#lzdifforderallbindings]


All eight bindings now have the test, each built so it cannot pass by accident: arrival order and canonical order are constructed to genuinely differ, that difference is asserted **explicitly** before the sort is checked, and both mutations (reverse, delete) redden exactly one test and nothing else.

| binding | sorted before? | commit |
|---|---|---|
| rs, go, py, js, kt, cs, dart | yes | `eedeaa8` `34cb846` `e52a78a` `46a474b` `6e2fd0f` `6b3f625` `46e3c3c` |
| **cpp** | **no** | `c4b26b3` — library fix |

**lazily-cpp returned raw log order.** `diff` was a bare filter over the log with no sort, so it produced `[(1,1) (2,1) (3,1) (4,1) (3,2)]` where every other binding produces `[(1,1) (2,1) (3,1) (3,2) (4,1)]`. That is a genuine cross-binding divergence in how `deliver.only: [0, 2]` resolves — and it was **latent, not live**: `non_contiguous_anti_entropy.json` has replica `a` produce all three ops locally with nothing remote interleaved, so its log is already monotonic and the two orders coincide there. It becomes live the moment any fixture interleaves a lower-counter remote op. The agent fixed the library rather than pinning the wrong behaviour, which was the right call.

**Two bindings independently reproduced zig's measurement.** In cpp and in dart, deleting the sort left the anti-entropy fixture *passing* — 706 other dart tests green, `LosslessTreeConformance` green in cpp. So the corpus genuinely cannot police this, in three separate languages now.

**rs found something bigger than the task.** `mod lossless_tree_crdt` is `#[cfg(feature = "lossless-tree")]`, `make test` runs default features, and the `test-lossless-tree` target enumerated only `--test` binaries — so **all eight of that module's unit tests compiled to nothing** under `make check`. Invisible to the runtime conformance manifest, because unit tests open no fixture. Fixed by adding `--lib lossless_tree_crdt::` to the target and the CI step; the repo's own `ci-reach` guard caught the half-fix when I added the recipe line without the CI line, which is exactly what that guard is for.

Two smaller notes. cpp's new configure-time guard from `d052c4f` fired on the agent's *comment* explaining why the new file is not a conformance suite — it greps raw source including comments. Real hazard, worked around by rewording, worth knowing. And two agents hit a force-push guard misfiring on plain fast-forwards, working around it with a different refspec form.

One divergence found and deliberately **not** fixed, now filed: cpp's `apply_update` never advances its Lamport counter, where rs/go/py/zig all do `counter = max(counter, op.id.counter)`, and it declares a `buffered_` member `apply_update` never uses — so cpp has no dependency-buffering retry for out-of-order ops. Neither is exercised by the current corpus.

Exchange is at 871 lines, accretion `block`; queue is active so I keep draining per its guidance.

### Re: #lzjsscenarioswallow — the swallow was not just anonymous, it accused the wrong component — opus-5

> **Queue prompt:**
>
> do [#lzagentloopcibilling]


> **Queue prompt:**
>
> do [#lzjsscenarioswallow]


`check-scenario-coverage.mjs` dropped an unparseable corpus fixture with a bare `catch { continue; }`. I filed it as "the only unnamed swallow in that ladder". Measuring it turned out worse than that.

Dropping the fixture from the **corpus** side left the ledger's genuine replays of its scenarios looking like phantoms, so the script blamed the evidence channel:

```
ERROR: the ledger records a replay of scenario 'individual_disposal' in
       'reactive-graph/scope_teardown_equals_fold_of_disposals.json', which the
       canonical corpus does not carry. The recorder attributed a replay to
       something that is not a corpus scenario ...
```

The recorder was right. The corpus file was truncated. And the count slid from `153/153` to `151/151` with nothing naming the file — so an operator reading that output would go looking at the manifest recorder, which is the one part that was working.

The exit code was always correct, which is why this was filed as diagnosability. But "unnamed swallow" undersold it: a check that loses an input and then reports the *consequence* as a fault in a different subsystem is worse than one that says nothing, because it sends you somewhere. It now names the fixture, calls it PRESENT but unusable, distinguishes that from the absent-corpus case this script legitimately skips on a sibling-less checkout, and tells a perturbation probe that truncating a fixture perturbs nothing a runner can disagree with.

**Also removed `listFixtures()`** — exported from the corpus seam, zero call sites, and the exact primitive that introduces the enumerating-host shape `#lzledgeragreementaudit` exists to catch. lazily-cs died on that shape when a fixture landed upstream: a bare `KeyNotFoundException` naming neither the fixture nor the reason. Every js runner NAMES its fixtures, which is why a new upstream fixture reddens the not-opened rung instead of crashing a runner — and keeping an unused enumeration helper around is an invitation to reach for it. The comment left in its place says what to add first if one is ever genuinely needed: the drift assertion, before reading a byte.

Proven: control on a byte-identical scratch corpus exits 0; a truncated fixture now exits 1 naming the file; the committed version on the same input produced the misdirected message above. `make check` OK, 627 tests. `lazily-js@98689c8`, pointer pushed.

Exchange is at 897 lines, accretion `block`; queue is active so I keep draining per its guidance.

### Re: #lzagentloopcibilling — diagnosed to the mechanism, gated because only you can clear it — opus-5

I cannot settle a payment, so this one is genuinely operator-gated. What I could do was make the action cheap and bound the exposure exactly, rather than hand you "CI is broken".

**The mechanism, confirmed rather than inferred.** `btakita/agent-loop` is **private**, so it consumes paid Actions minutes. `btakita/tsift`, `btakita/agent-doc` and every `lazily-hub/*` binding are **public** and run on free minutes. That is precisely why binding CI has been green all session while the superproject has been dark — not a coincidence, and not something I should have read past as long as I did.

**The window.** Last successful run `31293624224` at `2026-08-09T04:00:25Z`; first billing failure `31297858292` at `2026-08-09T05:58:50Z`. So it started in a ~2-hour window on Aug 9 and has been down about two days. Both workflows are affected, and `btakita/agent-doc`'s current red is a *real* test failure, not this — I checked rather than assuming a common cause.

**What has actually gone unverified**, which is the part that matters:

| lost | covered elsewhere? |
|---|---|
| `agent-doc.yml` → `check` (`make check` under tmux) | **no** |
| `agent-doc.yml` → `tmux-ci` (`make tmux-ci`) | **no** |
| `lazily-network-suite.yml` → tiered `network-suite-*` | **no** |

None of that is reachable from the per-binding CI that has been green. So every submodule-pointer commit I pushed this session is verified at the *binding* level and unverified at the *superproject* level.

**What unblocks it, and nothing else does:** open Billing & plans for the account owning `btakita/agent-loop`, settle the failed payment or raise the Actions spending limit, then re-run one workflow and confirm a job actually starts (the tell is a job with steps, rather than an empty one with no logs). Once it does, re-run `make check` and `make tmux-ci` on HEAD before trusting the pointer commits from the outage window.

I have gated the item rather than leaving it as a drainable head — it would otherwise churn a no-op cycle every pass, and the queue would keep offering work no agent can do. The item now carries the run ids, the timestamps, the private-vs-public mechanism, and the coverage table, so the action is a two-minute one when you get to it.

Five drainable heads remain, all agent-workable.

Exchange is at 925 lines, accretion `block`; queue is active so I keep draining per its guidance.
<!-- agent:boundary:067ed016:lazily -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-commit-push use subagents for fan-out tasks" priority go -->
- ~~do [#lzdartvacuousfixtures]~~
- ~~do [#lzgoassertkeygaps]~~
- ~~do [#lzpystdlibfloorsunread]~~
- ~~do [#lzconvergedlistlength]~~
- ~~do [#lzunboundblockguard]~~
- ~~do [#lzcorpusrootguards]~~
- ~~do [#lzrsspecdirconsts]~~
- ~~📌 do [#lzperturbaudit]~~
- ~~do [#lzoverrideallrunnersaudit]~~
- ~~📌 do [#lzzigdiffmutant]~~
- ~~📌 do [#lzledgeragreementaudit]~~
- ~~📌 do [#lzzigingressspecdir]~~
- ~~📌 do [#lzspecschemasoverride]~~
- ~~do [#lzcorpusabsencehandling]~~
- ~~do [#lzdifforderallbindings]~~
- ~~do [#lzjsscenarioswallow]~~
- ~~do [#lzagentloopcibilling]~~
- do [#lzpystdlibmutants]
- do [#lzjsblocknamemasking]
- do [#lzcsuncomparedvalues]
- do [#lzktgradleinputfingerprint]
- do [#lzcppapplyupdatelamport]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#lzgdvocab] lazily-gd's reactive-graph runner is missing the fanout/churn/dispose_fanout/dispose_stale_handle bulk ops AND real id recycling, so 2 canonical fixtures remain: churn_returns_to_baseline.json and recycled_id_inherits_nothing.json. Recycling is the hard half and is a DESIGN decision, not runner vocabulary: Godot instance ids are monotonic and never alias, so 'a recycled id inherits nothing' has no natural referent — the binding needs an explicit id layer (a free-list over slot indices with a generation tag, the shape lazily-rs's SlotId uses) before the fixture means anything. Removing both entries from KNOWN_UNCOVERED in scripts/check-conformance-coverage.sh is the definition of done. DONE already: the merge_cell group (5 fixtures) landed with LazilyMergePolicy + Source.merge + ctx.source_with, and GDScript is now ~ on merge-algebra.
- [ ] [#lzpystdlibmutants] lazily-py does not APPLY the stdlib mutation ledger, unlike lazily-rs. Each stdlib fixture carries a `mutations` array of {operator, must_fail} declaring that mutating the implementation a named way MUST make the listed scenarios fail. tests/test_stdlib_conformance.py checks only that the ledger is well-formed (non-empty must_fail, ids subset of the fixture's own scenarios) and, since #lzpystdlibfloorsunread, that it meets mutation_floor. It never mutates anything, so rebinding an operator to an unrelated scenario stays green and the ledger's central claim is untested. lazily-rs does this properly: tests/stdlib_conformance.rs replays each operator through an INDEPENDENT interpreter (independent_failures(path, &fixture, Some(operator))) and asserts must_fail is a subset of the scenarios that actually failed under that mutation — an independent implementation is what makes the mutation applicable without perturbing the production code under test. Port that shape: add an independent interpreter for timer/timeout/revision_barrier, parameterised by mutation operator, and assert every declared must_fail scenario really fails under its operator and passes without it. Definition of done: rebinding a mutation's must_fail to a scenario the operator does not break reddens, and so does an operator no interpreter implements. Audit go/js/cs/dart for the same gap while there — the perturbation audit found go's ledger is checked the same self-referential way.
- [ ] [#lzjsblocknamemasking] lazily-js records assertion keys as fixture+block+key with BARE block names, so sibling blocks mask each other: frames[3].assertions and frames[0].assertions collapse to the same record, and a key asserted in one marks the other satisfied. Measured during #lzunboundblockguard: path-qualifying the block names de-masks 42 further findings — 20 keys never consumed by any runner, 16 read but never asserted against the fixture's own value, and 12 object-valued keys with no key-set check. The bind-ledger walk added in 2d0bfbd is already path-qualified; it is the KEY records (scripts/check-assertion-keys.mjs and test/support/conformance-manifest.cjs) that still collapse. Path-qualify them, re-pin the floors, then work the 42. Definition of done: two sibling blocks carrying the same key name report independently, and each of the 42 is either fixed or carries a declared reason.
- [ ] [#lzcsuncomparedvalues] lazily-cs cannot yet detect an assertion callback that receives the fixture value and never COMPARES it — direction (b) of #lzunboundblockguard, deliberately not built there. The live example was AssertKeyWith("nonterminal_outcomes", want => Assert.All(want.EnumerateArray(), item => !ParseOutcome(item).IsTerminal())): it READ every element, so a recording wrapper would not have caught it; it simply never compared them to the projection, and Assert.All over an empty sequence is vacuously true. Two mechanisms were spiked and measured. A recording wrapper misses this exact case and needs ~186 call-site edits. Differential re-execution (perturb the value, require the callback to fail) needs zero call-site changes and DOES catch it, but fails structurally here: this repo's runners record divergences into a ledger via Check(...) rather than throwing, so a perturbed re-run signals nothing and appends phantom divergences — measured 8 broken tests and 288 false 'insensitive' keys. The real fix is to give the divergence-recording runners a comparison seam the tracker owns, so a comparison is observable rather than inferred. That is a runner change, not a guard change. Definition of done: a callback that reads the fixture value without comparing it to replayed state fails, and the existing divergence-recording runners keep working.
- [ ] [#lzktgradleinputfingerprint] lazily-kt's Gradle test-task input wiring does not do what its comments claim, in two places. (1) A Test task's `environment` is UNTRACKED and PathSensitivity.RELATIVE means a byte-identical corpus at a different path fingerprints the SAME, so the comment added in ba7f41d claiming that forwarding the env var 'makes the variable part of the task's input fingerprint' is false as written — it holds only when content differs. Measured on the schemas input during #lzspecschemasoverride: pointing at a byte-identical scratch copy produced '11 actionable tasks: 11 up-to-date, BUILD SUCCESSFUL in 314ms', so the run meant to prove the override took effect never started a JVM. Fixed for SCHEMAS in d8b6c08 via inputs.property; the CORPUS input still has it. (2) CORRECTION, measured in 22e75e0 (#lzcorpusabsencehandling): d8b6c08's own rationale ALSO claims a missing directory under an explicit override 'fingerprints the same as no sibling at all (both absent, because the input is optional), so a probe pointed at a nonexistent tree could be answered from a cached GREEN'. That case cannot occur — a missing input directory fails Gradle task validation outright, before any test runs, and `optional(true)` does not change it (optional excuses an absent PROVIDER VALUE, not a nonexistent directory; the provider always has one via orElse). The inputs.property is still justified by the byte-identical-copy half of its argument, so nothing is broken — but the missing-directory half of that comment must be deleted rather than left as folklore. Do both: add inputs.property for the corpus dir and re-prove that a byte-identical corpus copy RE-RUNS rather than reporting UP-TO-DATE, and correct both comments to state only what is actually tracked. The specSchemas input also still carries an inert optional(true) worth removing for the same reason 22e75e0 removed the corpus one.
- [ ] [#lzcppapplyupdatelamport] lazily-cpp's LosslessTreeCrdt::apply_update diverges from every other binding in two ways, found while pinning the diff order (#lzdifforderallbindings) and deliberately left unfixed there. (1) It never advances the Lamport counter: rs, go, py and zig all do counter = max(counter, op.id.counter) when applying a remote op, so a subsequent local op is guaranteed a counter above everything it has seen. cpp does not, so a replica that receives a higher-counter remote op and then makes a local edit can mint an id that collides with or sorts before an op it already holds. (2) It declares a buffered_ member that apply_update never uses, so cpp has NO dependency-buffering retry for an op whose parent has not arrived — the other bindings buffer and retry, which is what makes delivery order-tolerant. Neither is exercised by the current corpus, which is why both survived: non_contiguous_anti_entropy.json delivers a gap but never an op whose parent is missing, and no fixture makes a local edit after receiving a higher-counter remote op. Establish the intended semantics from lazily-spec, fix cpp, and add a canonical fixture that discriminates BOTH — a scenario where a replica edits after receiving a higher-counter remote op, and one delivering a child before its parent. Definition of done: the new fixtures redden pre-fix cpp and stay green in the other nine bindings, and cpp's apply_update matches the reference semantics.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] 🚧 [#lzagentloopcibilling] [operator-action] GitHub Actions is blocked on btakita/agent-loop by a spending limit, and has been since 2026-08-09. Every run of both workflows (Agent Doc, Lazily network suite) reports "The job was not started because recent account payments have failed or your spending limit needs to be increased" — no job starts, so there are no logs and nothing to debug in the workflow itself. Diagnosed precisely: btakita/agent-loop is PRIVATE, so it consumes paid Actions minutes; btakita/tsift, btakita/agent-doc and every lazily-hub/* binding are PUBLIC and run on free minutes, which is why binding CI has been green throughout while the superproject has been dark. Last successful run 31293624224 at 2026-08-09T04:00:25Z; first billing failure 31297858292 at 2026-08-09T05:58:50Z. COVERAGE LOST for that whole window: the `check` job (make check under tmux) and the `tmux-ci` job (make tmux-ci) in agent-doc.yml, plus the tiered `network-suite-*` targets in lazily-network-suite.yml — none of which is covered by the per-binding CI that has been green. OPERATOR ACTION, and nothing else unblocks this: open Billing & plans for the account that owns btakita/agent-loop, settle the failed payment or raise the Actions spending limit, then re-run one workflow and confirm a job actually starts. Until then treat superproject CI as ABSENT rather than passing, and do not read a green binding CI as coverage for it. Once jobs start again, re-run make check and make tmux-ci on HEAD before trusting the backlog of submodule-pointer commits pushed during the outage.
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
