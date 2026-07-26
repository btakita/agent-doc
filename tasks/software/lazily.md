---
agent_doc_session: d818612d-3614-4c43-b5ec-83923406aa46
resume: 24f78fda-2db5-41b6-8eb1-52772d913df8
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
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/44ae7f3a5c03b112ffe17aa5c85d000e3d79a6e9a4875e16442309ebf1c06a1c-20260726-090849.md`*

Compacted content:
- Archived 28 response topic(s): create lazily-cs (C#); complete all lazily-cs features; queue + backlog attributes; 25 more
- Prior summary/context: /goal Please create lazily-cs (C#) implementation. Create gh lazily-hub/lazily-cs. Add a submodule to src/lazily-cs. ...

### Re: the done-archive quote bug is fixed, not worked around — opus-5

> **Queue prompt:**
>
> do [#lzdonequoteattr]


`#lzdonequoteattr` is done: `src/agent-doc` `32583dbcb`, pushed, `make check`
8152/8152, installed, controller marked to recycle at the next idle boundary.

**The defect.** `parse_attrs` kept attribute values verbatim, quotes included, on
the stated assumption that "values are unquoted (no quote support needed for
simple mode values)". That assumption was false about agent-doc's own writers: a
compact wrote `archive="tasks/software/lazily.done.md"` into this very document,
and from then on every preflight and session-check hard-failed with `must point to
a .done.md file` — because the parsed value ended in a quote character, so the
`.done.md` suffix check could never match. Earlier this session I only unquoted
the marker by hand to unblock the cycle; that was a workaround, and the parser
would have broken the next document a compact touched.

Values may now be quoted with `"` or `'`, and quoting also does what quoting is
for: `preset="two words"` no longer truncates to `two` under whitespace splitting.
An unterminated quote takes the rest of the input rather than failing closed —
one stray quote unparsing an entire marker is the same class of outage.

The fix lives in `parse_attrs`, so both done-archive validators pick it up from
one place; they already read through `attrs`.

**One finding worth recording: a test was pinning the defect.**
`parse_patch_with_transfer_source_attr` asserted the value *with* its quotes. The
buggy behaviour was the expected behaviour, which is a large part of why this
survived — anyone touching the parser would have seen a green test telling them
quote retention was intended. `transfer-source` turns out to have no production
consumer that wanted the quotes; the one attribute that did care was breaking
documents. I corrected the assertion and said why in the diff rather than quietly
flipping it.

**Coverage, mutation-checked.** Four `parse_attrs` unit tests (both quote
characters, quoted-value-with-spaces, quotes not disturbing following attributes —
using the real `preset="#spec-test-commit-push" priority go` marker from the
document that broke, and an unterminated quote) plus three against the actual
resolver: a quoted archive resolves, quoted and unquoted agree, and a
non-`.done.md` path is **still** rejected so the fix did not widen the check.
Reverting the strip turns four of five and two of three red respectively, and the
"still rejected" test correctly stays green — it should not depend on quoting.

**Two notes on the shared tree.** The first full-suite run failed on
`finalize_integration` with `[double-spawn] failed to exec … No such file or
directory` — the test binary vanished mid-run, which is the concurrent-build
symptom, not a regression; it passes in isolation and the re-run was clean. And
`src/agent-doc` still carries another session's in-flight work
(`.claude/skills/agent-doc/SKILL.md`, `.claude/settings.json`, `.codex/`,
`.opencode/`). I staged only my three source files and left all of it alone.

**Also confirmed since last cycle:** you published js, so
`@lazily-hub/lazily-js@0.29.0` is live with `latest` pointing at it. That closes
the release — all nine bindings published. The CI OIDC job for that repo will keep
404ing on `PUT` until its npm trusted-publisher config is finished, so treat a red
`Release` run there as expected rather than a regression and check npm directly.

Next head is `#lzqfspec` — Phase 0 of the queue-family plan.

### Re: Phase 0 — the queue family's Core surface is pinned — opus-5

> **Queue prompt:**
>
> do [#lzqfspec]


`#lzqfspec` done. `lazily-spec` `63e07b4`, pushed, `make check` green; the ten
sibling READMEs regenerated and pushed; superproject at `edf3d6774`.

**The Core/Extended split is evidence-based, not taste.** Normalising for naming
convention across the eight bindings that ship the family, nine `QueueCell`
methods are present in **8 of 8** — `try_push`, `try_pop`, `close`, `head`, `len`,
`is_empty`, `is_full`, `is_closed`, `capacity` — so they are laws. The rest are
named Extended explicitly, which is what stops their absence being scoreable.

Three Core entries carry weight that is easy to miss, so I said why in the spec:

- **`is_empty` is Core here and Extended on `ReactiveMap`.** On the map it is
  `len() == 0`. For a queue it is a named reactive observable the threading
  section already requires, and the reader-kind independence law is stated over
  the whole `head`/`len`/`is_empty`/`is_full`/`closed` set — drop one and the law
  becomes unassertable. Called out so the difference does not read as sloppiness.
- **A polled version counter is not a reader kind.** It registers no dependency
  edge, so nothing can be invalidated by it and the independence law cannot be
  observed. Snapshot diffing likewise. Both must be recorded as divergent rather
  than green — which is exactly what zig and js were doing while scoring `✅`.
- **`try_pop` must distinguish empty from closed**, or the closure lifecycle law
  collapses into a bare optional.

**Reader-kind derivation is pinned to memoized-derive with explicit
invalidation** — settled by the precedent the ReactiveMap work established across
all nine bindings, plus the fact that a version-cell design charges a graph write
on every op even with no subscriber, which is precisely the cost the
demand-driven clause exists to avoid. A version-cell implementation stays
conformant if reads register an edge and the invalidation set is exact; polled
counters never are.

**Two flavor questions settled here rather than per binding.** `TopicCell`
broadcast needs nothing extra under a thread-safe context — each subscriber owns a
cursor, cursors only advance, so reads commute and retention is a monotone
function of the cursor set; confluent by construction. And `WorkQueueCell` lease
expiry stays caller-driven on every flavor: `reap_expired` takes the time as a
parameter, and a flavor that grew its own clock would be unreplayable for no gain.

**The third question answered itself empirically.** Across all eight bindings the
queue-family sources contain **no** `async fn`, `suspend fun`, `Future`, `Task`,
`Promise`, or `await` — anywhere. `pop` is uncoloured everywhere, so the spec now
forbids a flavor from colouring it. That is measured, not argued.

**The threading contract is now normative.** It named `ThreadSafeContext` as the
route to cross-thread MPSC while no binding had a constructor that accepted one —
so for the entire v1 line the only documented path to that guarantee was
unimplementable. Recording a capability as absent is fine; documenting a route to
it that does not exist is not.

**`coverage.json`'s three conflated queue rows are now nine** (primitive ×
flavor), and they say what is true rather than what is flattering: single-threaded
ships in eight bindings but is **partial** in zig (reader kinds are `u64` counters
callers poll) and js (no context at all — invalidation is a snapshot diff), and
**no binding ships a thread-safe or async flavor of any queue primitive**. The gap
is now visible in the matrix instead of hidden behind three green rows.

**And the matrix stops contradicting itself about Dart.** `docs/coverage.md`
declared the thread-safe context "not applicable" for JS and Dart while both
shipped the layer and passed fixtures — I sharpened that contradiction last cycle
by scoring dart's map flavor `✅`. The carve-out was a design argument, not a
scoring fact, so it is withdrawn and the rows are scored as shipped. The *design*
question — whether those bindings should expose the flavor at all, given Dart
ships a duplicate weaker than what it duplicates and JS's README contradicts its
own source comment — is recorded as **still open** rather than silently answered.

Next head is `#lzqfformal` — Phase 1, the Lean theorems that justify
flavor-neutrality instead of assuming it.

### Re: Phase 1 — flavor-neutrality is proven, not assumed — opus-5

> **Queue prompt:**
>
> do [#lzqfformal]


`#lzqfformal` done. `lazily-formal` `036d2a8` and `lazily-spec` `dad19e5`, both
pushed; `make check` green in both, and lazily-rs's `make test-lazily-formal`
cross-check green. Superproject bumped.

**The gap.** `QueueCell.lean` already models storage — elements, closed,
push/pop/close — and proves storage laws. It says nothing about reader kinds. Yet
every binding's queue shell carries the same load-bearing comment: on a successful
op it bumps *exactly* the reader kinds whose value provably changed, computed from
the transition rather than by deriving each value. That claim is what makes the
plane portable across flavors, and it was asserted in comments in lazily-rs,
lazily-zig, and lazily-kt and **proven nowhere**.

Eleven theorems in a new `QueueReaderKinds.lean`, rather than by editing
`QueueCell.lean` — the storage theorems keep their shape and the reader-kind plane
stays a separate concern.

**The one that licenses the whole plan** is `invalidationSet_no_derive`: the set is
a function of the bound, the pre-op length, emptiness, and the closed flag
**alone**. Two queues agreeing on those agree on the set whatever their elements
are. That is the "not by deriving the value" half stated formally, and it is
precisely what makes running the *identical* rule on all three shells sound — the
rule consumes only transition data, and no flavor colours transition data.

The rest:

- **Failed ops invalidate nothing, in both directions.** A rejected push and a pop
  on empty emit the empty set *and* change nothing. A binding that invalidated on
  a rejected push is over-invalidating against this.
- **The reader kinds do not leak into each other.** `close` moves `closed` and
  leaves the elements alone, so `head`/`len`/`isEmpty`/`isFull` cannot move; push
  and pop never touch `closed`.
- **`apply_atomic`** — a reader seeing post-op storage sees every kind in the set
  cleared and every kind outside it untouched. There is no intermediate state
  where storage advanced but only part of the set applied, which is exactly what
  would let a subscriber observe `len` bumped while `isFull` had not yet flipped.
- **`applyInvalidation_order_independent`** is the queue-family instance of the
  thread-safe confluence obligation the plan asked me to cross-check. Invalidation
  consults only *membership*, so two threads bumping the same set in different
  orders converge — the set is a set, not a sequence. `ThreadSafe.lean`'s
  `flushBatch` gives the analogous property for a graph batch; this states it over
  the plane the queue rule actually emits, so the thread-safe shell needs no extra
  machinery. Plus idempotence: replaying a transition cannot deepen the
  invalidation, so at-least-once delivery of a bump is safe.

**No per-flavor models**, as the plan requires. The point is that the algebra does
not fork; a per-flavor copy would be the defect this module exists to rule out.
One model, three shells.

**Mutation-checked, because a Lean file that compiles is not evidence its theorems
say anything.** Making `invalidationSet` read the elements breaks
`invalidationSet_no_derive` (4 errors); letting a rejected push still invalidate
breaks the not-applied pair (3 errors). Both restore clean. That is the same
discipline the ReactiveMap gates needed — and there it caught four false greens.

Two notes on getting there. This project is **core Lean 4 with no Mathlib**, so my
first draft failed on Mathlib lemma names (`List.length_eq_zero`, `decide_False`)
and I rewrote the proofs in the surrounding style. And `apply_atomic` initially
left a bridge goal because `simp` normalises `List.contains` to `∈`; stating it
with membership directly removed the friction rather than papering over it.

`cell-model.md` now cites both models side by side, so a reader arriving at the
storage-agnostic claim sees where the reader-kind half is pinned.

Next head is `#lzqffx` — Phase 2, wiring the queue fixtures into every binding's
runner against all three flavors. That one lands **before** any implementation, so
the gap goes red first.

### Re: Phase 2 — the audit is clean, and the ledger enforces itself — opus-5

Two deliverables, and one honest remainder.

**The cross-binding audit the plan asks for is done, with a clean result.** Phase
2 item 3 says to grep every queue runner for the `invalidates`-nesting trap — the
one that made lazily-rs's *map* runner read `invalidates` off the step, always
find `None`, and never run its assertion once. It is **not present in any queue
runner**: rs, py and kt all read `expected.invalidates` correctly. A negative
result, recorded on the item so nobody spends the time again.

The related check turned up one thing worth knowing. **7 of 8 bindings genuinely
replay the canonical queue corpus** (rs, py, go, dart, zig, kt, js). lazily-cpp
does not — `test_queue.cpp` transcribes five fixtures by hand and never opens the
canonical bytes. But it is **honest about it**: its coverage script lists exactly
those files under `KNOWN_UNCOVERED` with the reason "mirrored by hand … which
never open the canonical bytes." That is the right pattern, not a false green, so
I have filed it as `#lzqfcppfx` rather than treating it as a defect found. My
first read of that grep said "no fixture load" for go as well, which was wrong —
go uses the shared `loadCollectionFixture` from a sibling file.

**The reference ledger ships in lazily-rs** (`d9e8851`, pushed).

The plan asks for a three-flavor replay with a declared skip ledger — but no
binding ships a thread-safe or async queue primitive, so a three-flavor replay
written today would skip two of three flavors entirely. A suite that skips almost
everything while reporting green is the exact failure this whole phase exists to
prevent, so a *declared* ledger was not good enough.

**So the ledger is wired to the source, not to a comment.** It greps `src/` for
each unshipped flavor's type name. The moment Phase 3 adds `ThreadSafeQueueCell`
or `AsyncQueueCell`, the test goes red, names the runner to extend, and explicitly
warns against flipping the flag alone. It fails in the other direction too —
claiming a flavor is shipped when its type is absent is also red — so the ledger
can neither rot nor overclaim. Both directions mutation-checked: appending a stub
`ThreadSafeQueueCell` turns it red, flipping `async` to shipped turns it red, both
restore clean.

Two further guards, each for a failure this family has actually had.
`ledger_is_not_all_skips` fails if every flavor is unshipped — zig's
reactive-graph runner established that rule, because "skipped" and "passed" are
indistinguishable in a summary line. And `shipped_flavor_replays_the_corpus`
counts fixtures, steps and invalidation matrices and requires all three non-zero:
an absence guard proves the corpus exists on disk, only a positive count proves
this binary opened it.

**Wired into `make check` deliberately.** Earlier today I found the collections
family gate had spent its entire life compiling to `running 0 tests` because no
make target ran it. An unwired gate and an unenforced ledger are the same defect,
so this one has its own target and `check` depends on it. `make check`: 634 test
suites ok, 0 failed; only `benchmark-check` stops it, with the documented
environmental "no Criterion estimates found".

**The remainder is honest, not a stall.** `#lzqffx` stays open with what is done
and what is left recorded on it: mirroring the ledger into the other seven
runners. The rs file is the template and the enforcement trick is the whole
point — that part is done once, and the rest is mechanical.

Next drainable head is `#lzqfrs` — Phase 3 — but it is gated on this one
finishing, and I have left that dependency recorded on it.

### Re: do [#lzqfports] — opus-5

Not this cycle, and the reason is written into the item itself: "Dispatch only after #lzqfspec + #lzqffx + #lzqfrs are committed so each agent has a settled spec, a red per-flavor gate, and a reference implementation." #lzqfrs is not done. Draining #lzqfports now would send six subagents to implement thread-safe and async queue primitives with no reference implementation to copy — six independent guesses at a shape I have not yet pinned in rs. That is the situation the precondition exists to prevent.

#lzqfports reached the head because I called preflight three times while re-establishing state after a context break, and each call advanced the selection past #lzqfcppfx and #lzqffx. The backlog order is already correct (#lzqfrs precedes #lzqfports); only the selection drifted. Nothing was lost — all 42 ids are still queued.

What did land: **#lzqffx is complete.** Phase 2 mirrors the rs self-enforcing three-flavor ledger into the remaining seven bindings.

No binding ships a thread-safe or async queue primitive, and cell-model.md now makes both Core, so their absence is a conformance gap rather than an unfinished nicety. A three-flavor replay written today would skip two of three flavors, and a suite that skips almost everything while printing green is the exact failure this family keeps producing. So each ledger is wired to the source rather than to a comment: the moment a `ThreadSafeQueueCell` or `AsyncQueueCell` appears, the ledger goes red, names the runner to extend, and warns against flipping the flag alone. It fails in the other direction too — claiming a flavor is shipped whose type is absent is also red — so a ledger can neither rot nor overclaim.

Each carries the two guards for failures this family has actually had: the ledger cannot be all-skips (zig's reactive-graph lesson — "skipped" and "passed" are indistinguishable in a summary line), and a positive count of fixtures, steps, and invalidation matrices, because an absence check proves the corpus exists on disk while only a count proves *this binary opened it*. Each also pins the `invalidates` nesting under `expected`, since rs's own map runner once read a step-level copy that nothing consumed.

Two mechanism notes worth keeping.

**Zig uses `@hasDecl`, not grep.** Zig's tests live *in* the source file, so `@embedFile("queue.zig")` finds the ledger's own marker string literals and reports every flavor as present — the test failed against correct code. `@hasDecl` is immune and strictly stronger: it asks the compiler what the module declares instead of asking the bytes what they mention, and it turns an unshipped flavor's arrival into a compile error rather than a test failure. The other seven keep their tests outside the grepped directory, so grepping is safe there.

**The cpp vacuity guard earned its keep.** Its first version resolved `include/lazily` relative to the working directory; ctest runs from `build/`, so it read no headers. The guard turned that into a loud `FAIL: the ledger check would be vacuous` instead of a silent pass — which is precisely the job. Fixed by walking up from the working directory.

One honest limit. cpp's replayed-fixture count rose 88 → 93 because the ledger now *opens* the queue bytes. Those entries stay in `KNOWN_UNCOVERED` with a sharpened reason: opening is not replaying. The behavioural assertions are still a hand transcription in `test_queue.cpp` that can drift from the bytes it mirrors, and shrinking the list means making that file drive the fixture. That is #lzqfcppfx, the next drainable head.

Mutation-checked per binding — simulating each flavor's arrival turns the ledger red (zig: compile error), and all restore clean. All seven gates green: py, go, js, dart, kt, cpp, zig. Committed and pushed per binding, with superproject gitlinks bumped in `20e11d3aa`.

One note on that commit: `tasks/software/lazily.done.md` was already staged when I committed the gitlinks, so it rode along under the gitlink message. It is this session's own archive, not another session's work, so nothing was clobbered — but the message does not describe it.

### Re: do [#lzqfcppfx] — opus-5

> **Queue prompt:**
>
> do [#lzqfcppfx]


Done. lazily-cpp now replays the canonical queue corpus instead of transcribing it.

The old arrangement was subtler than "no coverage". `test_queue.cpp` had five blocks named after five canonical fixtures, and the *observation harness* in them was sound — real derived `Computed` readers, real validity checks, the right invalidation polarity. What was wrong was that the **data** was a copy. A copy cannot drift-detect: add a step upstream, tighten one `invalidates` entry, change an expected value, and the transcription stays green against the contract it was written for rather than the one the spec holds today. Hence the honest KNOWN_UNCOVERED listing — declared, but declared as a gap.

`test_queue_conformance.cpp` opens the bytes. Everything it asserts is library-sourced: the five reader kinds come from derived `Computed` readers so an edge is genuinely registered rather than a counter polled, and the `invalidates` matrix is checked against whether those readers were *actually* invalidated — captured before any refresh, because reading revalidates. It reads `invalidates` from under `expected`, where it lives; a runner that looked at step level would find nothing and check nothing, which is the trap that disabled rs's map assertion.

`elements` needed an actual decision rather than a default. The queue exposes no enumeration, so checking full contents mid-fixture would mean asserting against a model the runner maintains itself — that tests the runner, not the library, and is the fourth false-green shape from the plan. Instead every step pins head+len from real reads, and each fixture ends by draining the queue and comparing the popped sequence to the fixture's final `elements`. That is the only spelling where the contents claim comes from the queue.

Two findings worth keeping.

**A real API narrowing.** The fixtures use `pop` with an `Empty`/`Closed` status return, but cpp's `pop()` convenience wrapper collapses both to `nullopt` and cannot express it. The contract survives on `try_pop`, so both spellings replay through that. The wrapper is lossy against the spec — not a bug I fixed here, but worth knowing before the thread-safe and async flavors copy its shape.

**The coverage gate caught me.** My edit to shrink KNOWN_UNCOVERED took out `mergecell_algebra.json` as collateral, and the gate immediately failed with "exists on disk but is never replayed". Restored — it is still hand-mirrored in `test_merge.cpp`, a separate job.

Mutation-checked three ways, all caught: dropping `bump_head` from pop (caught by the invalidates matrix *and* the head value), popping from the back so FIFO breaks (head + returns), and making close non-terminal — that last one returned `Ok` where the fixture says `Closed`, and left an element behind that **only the final drain caught**. If I had settled for head+len, mutation 3 would have half-passed.

The five transcriptions and their now-unused Reader harness are deleted. What remains in `test_queue.cpp` is genuinely local — storage-backend seams, the minimal raw-channel contract, topic tests — none of it in the corpus.

`make check` green: 93 canonical fixtures across 18 areas, KNOWN_UNCOVERED 25 → 20. Pushed as `36bcb6e`, gitlink `4bc11979e`.

Two housekeeping notes carried by this cycle. `tasks/software/lazily.done.md` had briefly been committed as a stray full copy of this session document (frontmatter and all) when it rode along on an unrelated staged commit; the correct 18-line archive is restored here. And #lzqfports keeps a written kept-open reason rather than a silent skip: #lzqfspec and #lzqffx are committed, but its stated precondition #lzqfrs is not, so dispatching six binding subagents now would be six independent guesses at a shape rs has not yet pinned. #lzqfrs is the next real head.

### Re: do [#observerwordingcontradic] + do [#renametreesource] — opus-5

Both done, plus a recommendation on the decision item below.

**#observerwordingcontradic.** Fixed the eleven docstrings across cs, dart, zig, kt, js and formal. The distinction is worth stating because it is not cosmetic: an observer is a callback held in a registry that survives invalidation, an Effect is a graph node whose edges are re-discovered on every run. The one-line test — anything surviving an invalidation is not a graph edge — is how three registries were found hiding in lazily-py. Reference docstrings calling Effects observers are exactly how a reader concludes a registry is the intended shape and writes one.

I scoped this to the eleven sites the item names. The wider sweep for the word across all eleven repos is ~55KB, and most of it is ordinary English (`observe`, `observation`, "an observer never sees `len` bumped before `is_full`") or tests that assert observers are *absent*. Rewriting all of it would be an uncontrolled sweep, not this item.

**#renametreesource.** `cell_tree.rs`, `cell_tree.zig`, `CellTree.cs` and `CellTreeTests.cs` now match their types. The deprecated public aliases stay — `CellTree`/`CellTreeNode` still resolve and zig keeps a `cell_tree` module const pointing at the new file, following the deprecated-alias block already in `root.zig`.

One thing I checked rather than assumed. Zig discovers tests through `refAllDecls` from `root.zig`, so a rename that quietly dropped the file from the build would leave the suite green — the exact failure shape this plan keeps turning up. Grepping the output proved nothing (zig does not print passing test names), so I mutated an assertion inside `source_tree.zig`: 380 tests, 1 failure, named `lazily.source_tree.test.lazily/source_tree: per-node value isolation`. The file is genuinely in the run.

Also worth recording: the first zig run after the rename printed `failed command` while `rtk proxy` reported exit 0. Two clean re-runs at exit 0 showed it was a stale-cache artifact of the rename — but the masked exit code is the real hazard. Had I trusted the proxy's 0 I would have seen nothing; had I trusted the log line I would have chased a phantom. Both needed a raw re-run to separate.

**One pre-existing red, not caused by this change.** `lazily-rs make check` now stops at `benchmark-check`: `target/criterion` has been pruned, so `update-benchmark-results.py --check` finds no estimates. I reproduced it on a clean stashed tree — it fails identically without my edits. 634 test targets pass and there are zero test failures. I deliberately did **not** regenerate the benchmarks: that would write machine-specific numbers into `BENCHMARKS.md` as a side effect of a file rename. Filed as a backlog item, because a gate that cannot reach green is not a gate.

### Re: do [#recommendeddecidewhether] — opus-5

Kept open for your call, with a recommendation: **keep `*Cell`; do not rename the domain family to `*Source`.**

The reason is that the rename would assert something false. The v2 kernel renamed the two *value-node kinds* to `Source` and `Computed`, and kept `Cell` as the conceptual word for a value-bearing reactive node. A `QueueCell` is not a kind — it *composes* kinds: it owns five `Source<uint64_t>` reader-version cells and exposes derived readers over them. It is neither a source cell nor a computed cell, so `QueueSource` would name a kernel kind it does not belong to, and would specifically imply "written from outside, folds under a merge policy" — which is wrong for a queue with derived readers. The same argument holds for `TimerCell`, `LeaseCell`, `CircuitBreakerCell` and `PresenceCell`: each is a composite with its own pure core plus projecting cells.

So the vocabulary is already consistent. `Cell` = value-bearing reactive node (conceptual); `Source`/`Computed` = the two handle kinds; `*Cell` = a domain composite made of them. Three bindings flagged it as the largest remaining Cell-vocabulary surface, which is true, but size is not evidence of wrongness — and this one has to move in lockstep across nine bindings, so the cost of getting it wrong is nine repos of churn plus a deprecation surface on every public type.

I am not editing on this. It is a naming decision across nine bindings, and the standing rule here is to converge on naming before applying it. What unblocks it is a yes or no from you. If you do want the rename, the honest scope is: nine bindings in lockstep, deprecated aliases on every renamed public type, spec `cell-model.md` updated first so the bindings have something to conform to, and a decision on whether `MergeCell` collapses entirely — under the kernel it is already just `Source<T, M>` with a non-default policy, so it may not deserve a name at all.

### Re: do [#lazilyfailstarget] — opus-5

Fixed. `make check` reaches green in a clean checkout again, and the budget gate is stronger than before rather than weaker.

The problem was a gate that could not distinguish **"these budgets were never measured here"** from **"these budgets were measured and are red."** `benchmark-check` reuses whatever `target/criterion` holds, and hard-failed when it held nothing — so a fresh clone or a pruned target dir was red for a reason with nothing to do with the code. That is worse than it sounds: a gate that is always red stops being read, and then it is not guarding anything.

Missing evidence is now a loud skip — a banner saying NO BUDGET WAS ENFORCED, that this is the absence of a measurement rather than a pass, and how to populate it. `--require-evidence`, exposed as the new `benchmark-check-strict` target, keeps the hard failure wherever the budgets must actually hold.

The part that took the actual thought is that a broad skip would be an escape hatch: delete the directory, turn a red budget green. So the skip is deliberately narrow — it fires only when the criterion directory is **absent**. A directory that exists but yields no estimates is a broken or partial bench run, not a fresh checkout, and still exits 2. I verified all three rather than reasoning about them: absent → 0 with banner, absent with `--require-evidence` → 2, present-but-empty → 2. The instrumentation-profile check had the identical shape and got the identical treatment.

**A finding that is more serious than the bug I was sent to fix.** While confirming a local skip would not weaken CI, I checked what CI actually runs — and these budgets have **no CI enforcement at all**. `ci.yml` enumerates individual cargo test invocations and never calls `make check` or `benchmark-check`; `regressions.yml` runs only the loom model. So every budget in `BENCHMARKS.md` is enforced solely by whoever runs it locally, and until this fix that was nobody, because the gate hard-failed on a pruned target. The banner now says this explicitly instead of leaving the next reader to discover it, and I filed the real fix as a backlog item — a scheduled job that runs the benches and then `benchmark-check-strict`. Worth flagging there that the numbers are machine-specific, so that job needs a stable runner class or relative-delta budgets, not the absolute ones a laptop produced.

I chose the skip over the other option in the item — running the benches once to repopulate `target/criterion`. Repopulating would have fixed today and broken again at the next `cargo clean`, and it would have written this machine's numbers into the tracked table as a side effect of a bug fix. The structural change survives a pruned target; the item's own wording allowed either, and the banner still tells a developer to run `make benchmark-update` when they want real enforcement locally.

`make check` green end to end, zero test failures. Pushed as `74682c4`, gitlink `df732d1a0`.

One note on session size, since the queue is active and I am not stopping for it: the exchange is at 359 lines across 8 response sections, 13 cycles closed in the last half hour.

### Re: do [#lzqfrs] — opus-5

Phase 3's prerequisite question is now answered, and the answer removes work from the plan.

**ThreadSafeContext does not need a multi-root `clear_slots`.** The plan assumed four reader-kind handles meant four frontier walks and queued a new API for it — and, to its credit, told itself to verify rather than assume. `batch()` already closes it: `clear_slot` pushes into `state.batched_slots` while `batch_depth > 0`, and `finish_batch` sorts, dedups, and calls `flush_batched_invalidations` exactly once, handing every collected root to a single `clear_frontier_locked`. So the thread-safe queue primitives wrap their reader-kind bumps in `batch()` and need no new context API.

Reading the code is not proof, so it is pinned by `thread_safe::tests::batch_gives_clear_a_single_multi_root_frontier` and mutation-checked — making `finish_batch` flush once per collected root turns it red with "got 8".

**Two failed measurements, both worth keeping.** I did not get the observable right the first time, and each wrong attempt is a trap someone else would hit.

Counting recomputes of a shared downstream does not separate the cases at all. `computed` is guarded, so when the cleared nodes recompute to equal values the downstream is *correctly* suppressed — batched and unbatched look identical. My own vacuity control caught that one.

Comparing batched against unbatched does not work either, and this one is nastier: `SetCellInvalidation` is recorded only inside `flush_batched_invalidations`, so the unbatched path is not instrumented and that control read **0**. Had I written only the positive half, it would have "passed" while observing nothing — the exact failure shape this plan exists to prevent, produced by the person writing the guard against it. The discriminator that actually works is **root count**: one flush for four roots must also be one flush for eight.

**AsyncContext genuinely lacks both.** It has `source`, `computed_async` and `batch()`, but no synchronous-compute `computed` and no `clear` at all. Those are real and remain.

### Re: splitting the rest of Phase 3 — opus-5

I am not closing #lzqfrs, and I am not leaving it as one parent that gets gated after every partial step. Its verified half is recorded and removed; what remains under that id is the AsyncContext prerequisite alone. The six primitives are now three actionable ids — `#lzqfrsqueue`, `#lzqfrstopic`, `#lzqfrswork` — one per primitive pair, which is the per-phase decomposition rather than a tracking placeholder.

That split is not bookkeeping. It surfaces something the single parent id hid: **the thread-safe half of the queue work is unblocked right now.** Only the async half waits on the AsyncContext prerequisite, and a monolithic #lzqfrs would have made all six look blocked behind it.

Each successor carries the constraint that is easiest to get backwards: landing `ThreadSafeQueueCell` turns `tests/queue_family_conformance.rs` **red by design** — the ledger asserts that type is absent. The same change must flip the shipped flag *and* make the corpus genuinely replay against that flavor. Flipping the flag alone is precisely the failure the ledger was built to catch, so each item says so explicitly.

`make check` green end to end. Pushed as `a0c937a`, gitlink `2c86eba29`, plan findings recorded alongside.

### Re: do [#lazilythreadsafeworkqueu] — reference pair first — opus-5

I did not drain the head as given, and the reason is in the item's own text: it says "same per-flavor rules as #lazilythreadsafequeuecel", and that reference pair did not exist yet. Building the work-queue flavor first would mean inventing the rules it claims to follow, then retrofitting. So I built the reference — which is what actually unblocks the head.

**`ThreadSafeQueueCell` ships.** Reader kinds are memoized derives on its own graph, not polled version counters. That distinction is the spec's, and it matters: reading a counter subscribes you to a number, not to the queue, so a counter is not a reader kind at all.

Two things differ from the single-threaded shell, and both are forced rather than chosen.

**Storage is `Arc<Mutex<S>>` and invalidation runs with that lock released — for correctness, not throughput.** A reader's compute closure locks storage while the graph holds the context lock. An op that invalidated while still holding storage would take those two locks in the opposite order, and two threads doing both at once deadlock. So every op scopes its storage guard to a block that ends before the context is touched. `concurrent_push_and_read_do_not_deadlock` runs four writers against four readers, because a lock-order inversion is invisible in single-threaded tests and shows up as a **hang**, not a failure — nothing else in the suite would catch it.

**Multi-root invalidation goes through `batch()`**, needing no new context API. That is the prerequisite finding from the previous cycle paying off immediately.

### Re: the ledger, and two coverage gaps I had to close — opus-5

Landing the type turned `queue_family_conformance.rs` red exactly as designed — "flavor `thread-safe` now EXISTS in src/ but the ledger still records it as unshipped". The flag flipped to `shipped` in the *same* change as a replay that actually runs, because flipping it alone would silence the absence guard while testing nothing, which is the failure the ledger was built to catch.

Then two gaps that would have let this pass while under-testing.

**The step replay cannot see non-atomic invalidation.** Each reader ends up cleared either way; batching only changes how many frontier walks happened in between, and nothing inspecting state *after* the op can tell. Catching it needs an observer that runs *during* it, so `one_op_invalidates_reader_kinds_atomically` subscribes an effect to two kinds that transition together — a pop taking a full bounded queue off capacity moves `len` and `is_full` — and asserts it reruns exactly once. Mutation-checked: giving each clear its own batch turns it red. Without this test, glitch-freedom was simply unverified.

**The make target ran half the gate and said ok.** `test-queue-family-conformance` had no feature flags, so the new module cfg'd out entirely: **3 passed instead of 6**, reported green. I verified both counts before fixing it. This is the third time this session a gate has been able to report success while running less than it claims, and the only reason it surfaced is that I checked the count rather than the exit code.

Mutation-checked overall: dropping head invalidation on pop turns the corpus replay red; non-atomic invalidation turns the glitch test red. `make check` green, pushed as `46d81e4`, gitlink `20b460e81`.

The item is now narrowed to `AsyncQueueCell` alone, which is genuinely blocked: it cannot mint memoized-derive reader kinds until `AsyncContext` has a synchronous-compute `computed` and a `clear` — the remaining `#lzqfrs` prerequisite. I also recorded that the async replay will need a settle step, since derived async reads return `Option<T>` until resolved.

### Re: AsyncContext prerequisites + AsyncQueueCell — opus-5

Both done. The reference queue pair is complete and all three flavors replay the canonical corpus.

**`AsyncContext::computed` — a synchronous-compute derived cell.** `computed_async` resolves through a spawned task, so a read before resolution is `None`. That is right for a compute that awaits and wrong for one that cannot: a queue's `len`, `head` and `is_full` derive from out-of-graph storage and have nothing to await. Routing them through the task machinery would make every such read return `Option` and need a settle step to observe a value that was already known synchronously. These resolve inline on read.

The compute runs with `inner` **unlocked**, and that is load-bearing rather than an optimization. It is user code that reads sources, and those reads re-lock `inner` — running it under the lock deadlocks the context on its own reader. On completion the slot is re-checked instead of overwritten, since a concurrent reader may have resolved it first and both values are equal.

**`AsyncContext::clear` / `clear_slots`.** Invalidation was already multi-root internally — `invalidate_frontier_async` takes a slice — the context simply never exposed a way to hand it more than one root. That is the same shape as the ThreadSafeContext finding: the capability existed, the surface did not. `AsyncComputed::id()` is public because the roots of one invalidation are usually different types (`Option<T>`, `usize`, `bool`) and no single handle type spans them.

**`AsyncQueueCell`** then mirrors the thread-safe shell: `Arc<Mutex<S>>` storage with invalidation run outside the lock, memoized-derive reader kinds on its own graph, one multi-root clear per op.

Ordering is not async-coloured, and I made that claim falsifiable rather than asserting it: `reader_kinds_resolve_without_being_driven` reads every kind with nothing driven and nothing awaited. The corpus replay contains no settle step for the same reason — if those reads ever return `None`, that is the regression, not a cue to add an await.

### Re: what the mutations found — opus-5

**A gap none of the existing tests could see.** Dropping the forward `dependencies` list left all 34 async tests green, including the one I had just written to prove edges are registered. Invalidation travels the *reverse* edge, which `AsyncComputeContext::get` registers on the source itself — so my "does it subscribe?" test was satisfied by machinery I had not written. The forward list is what disposal walks to detach those reverse edges, so without it the graph invalidates correctly and then leaks a dangling dependent on every dispose. `sync_computed_records_the_edge_in_both_directions` now pins the direction nothing else was watching. I would not have found this by reading; only the mutation showed the test was weaker than its name.

**The feature-gate trap, twice in two cycles.** The make target needed `async` as well as `thread-safe`. Both flavors are cfg-gated, so the gate runs 3 tests with neither feature, 6 with thread-safe alone, and 8 with both — **exit 0 in all three cases**. I verified each count rather than trusting the exit code, which is the only reason it surfaced. That makes four times this session a gate could report success while running less than it claims.

Ledger flags flipped only alongside replays that actually run, both times. Mutation-checked: removing head invalidation on pop turns both async tests red. `make check` green — `21bce99`, gitlink `00ecc9273`.

Remaining in the family: `#lazilythreadsafetopiccel` and `#lazilythreadsafeworkqueu`. Both are now fully unblocked — the pattern they say to follow exists in `thread_safe_queue.rs` and `async_queue.rs`, and the context prerequisites they would have hit are gone.

### Re: do [#portconformancecoverage] — opus-5

Done, and one of the more interesting bugs of the session came out of it.

**The audit first**, since the item asked for it. Canonical corpus is 124 fixtures. Names referenced by each binding's tests: rs 111, py 114, js 121, go 115, dart 111, zig 94, cs 30. Only kt (117) and cpp (103) had a guard; the other seven could gain an unreplayed fixture and stay green forever.

**What landed is a static guard, and the header says so plainly.** It greps test sources for each canonical fixture's filename, so absence is caught — a fixture no test names cannot be being replayed. Presence proves nothing: a test can name a fixture in a comment and transcribe its contents by hand, which is *exactly* the drift found in cpp's queue tests. Only a runtime manifest proves the bytes were opened, which is what kt and cpp do. A green run here means "no canonical fixture is unmentioned", not "every fixture is replayed", and I have filed the upgrade rather than letting the weaker guard read as the strong one.

It fails in both directions — an unnamed fixture that is not excused, and a `KNOWN_UNCOVERED` entry naming a fixture the corpus no longer has, because a stale excuse is its own drift. It also refuses to run vacuously: reading no test sources is a failure, not a pass. Mutation-checked both ways.

The allowlists are seeded from the audit rather than invented, so each binding starts green *and* honest.

### Re: the bug that inverted the guard — opus-5

The first version reported **110 of 124 fixtures missing in lazily-rs**, while a manual grep found every one of them present.

The cause: `set -o pipefail` combined with `printf ... | grep -q`. `grep -q` exits on the first match, `printf` then takes SIGPIPE writing the remainder, and `pipefail` surfaces printf's death as the pipeline's status. So the check inverted — it reported every **covered** fixture as missing, precisely *because* the match succeeded. A guard whose verdict flips on the condition it is testing for is worse than no guard, and it would have been trivially "fixed" by padding the allowlist to 110 entries, which is how it would have shipped as a permanent lie.

It only surfaced because the count disagreed with an independent audit I had already run. That is the fifth time this session a gate has been wrong about its own result, and the second where the wrongness was invisible from the exit code alone.

Two notes on the state it exposes. **lazily-cs names 30 of 124** — that is the real state of the newest binding, not a guard defect, and it now cannot drift further without a deliberate decision; filed against the cs parity plan. And **lazily-dart had no Makefile at all**, so its verification was ad-hoc `dart analyze` plus `dart test` with nothing named `check`; it gets one, which is also why the guard had somewhere to hang.

All seven guards run green and are wired into each binding's `check`. Full `make check` verified end-to-end on js and cs; the guard target verified directly in the other five.

### Re: do [#lazilyupgradeconformance] — opus-5

> **Queue prompt:**
>
> do [#lazilyupgradeconformance]


Two of seven done, both verified clean, and the split is by language capability rather than by effort.

**js and py now prove the bytes were opened.** The static guard catches a fixture nobody mentions; it cannot catch one mentioned in a comment and hand-transcribed, which is the drift found in cpp's queue tests — source names the fixture, bytes are never read, check stays green against a contract the test only claims to follow.

The instrument matters here. js has ~73 `readFileSync` call sites and py ~70 load sites, and I did not touch any of them: js gets a `--require` preload wrapping fs reads, py a `conftest.py` wrapping `Path.read_text`/`Path.open`. That is the **stronger** instrument, not the lazier one — it records what the suite *really* read, so a runner that stops loading a fixture is caught even though its source still names it. Editing call sites would only ever record what each site claims to load.

A missing manifest now **fails**. It does not mean "no fixtures were read"; it means the suite ran without the recorder attached, and passing in that state is the vacuous green this whole line of work exists to prevent.

**Both bindings came back clean** — js opens 121 of 124, py 114 of 124, exactly matching their static counts. So neither had marker-only drift. That is worth having measured rather than assumed, particularly for js, which claims the highest coverage of any binding and where a false claim would have been the most misleading.

Mutation-checked twice each: removing the manifest fails with missing evidence, and deleting a single entry — a runner that still names a fixture but stopped reading it — fails naming that exact fixture. That second mutation is the case the static guard is structurally blind to, so it is the one that proves the upgrade is real rather than cosmetic.

One process note against myself: my first pass at the js script reported success with the old "static check — naming is not replaying" wording, because a heredoc-escaping mismatch meant that one replacement silently did not apply. The functional parts had all landed, so the guard was genuinely runtime — but its own success message was lying about what it had done. I checked which replacements applied rather than trusting that the script ran green, which is the same discipline that caught the `pipefail` inversion earlier in this cycle.

**The remaining five are a different problem, not a smaller one.** rs, go, dart, zig and cs are compiled and offer no interception seam, so each needs a shared fixture-loader helper plus migration of its existing load sites through it. That is genuine per-binding work rather than a bulk edit, so the item is narrowed to those five with the pattern recorded: record ids relative to the conformance root, append rather than truncate because several processes contribute, and fail closed on a missing manifest. Highest value is go (allowlist 9), then rs and dart (13 each); zig (30) and cs (94) claim the least, so a false claim there is the least misleading.

### Re: do [#lazilyconformancecoverag] — opus-5

> **Queue prompt:**
>
> do [#lazilyconformancecoverag]


I set out to work the cs allowlist down and instead found the number itself was untrustworthy, so I measured it properly first. The headline: **cs opens 37 of 124, not 30** — and my mid-cycle hypothesis about why was wrong in an instructive way.

**What I suspected.** cs has `StateChart.cs` *and* `StateChartConformanceTests.cs`, yet the static guard listed 7 statechart fixtures as uncovered. That pattern repeated for reactive-graph and materialization. The explanation is that cs discovers fixtures by enumerating the corpus directory — `Directory.GetFiles(dir, "*.json")` — so no fixture filename ever appears literally in its sources. A source grep was measuring the loader's shape, not the binding's coverage.

**What I expected to follow, and did not.** I expected the 94-entry allowlist to be largely false and the item I filed last cycle ("roughly three quarters unreplayed") to be wrong. It was not. Runtime measurement gives 37 opened against 30 named — the static guard **understated** coverage by exactly the seven fixtures reachable only by enumeration. So the substance of the original finding stands: 87 of 124 are genuinely unreplayed. I would have been comfortable asserting either conclusion from the code; only the measurement settled it, and it settled it against my expectation.

cs turned out to be the cheapest of the five compiled bindings to upgrade, not the hardest: every load already funnels through `SpecCorpus.Load`, so the shared-loader seam the others must build already exists here. No per-call-site edits.

**One trap worth recording, because its failure mode lies.** The manifest path must be absolute. The recorder runs inside the dotnet test host, whose working directory is the test project's output directory rather than the repo root, so a relative path writes the manifest where nothing reads it. What you then see is a green suite, an empty manifest, and a guard reporting missing evidence — which reads as "the recorder is broken" when the recorder worked fine and the path was wrong.

Mutation-checked both ways: removing the manifest fails as missing evidence, and dropping one entry — a runner that stops opening a fixture it still enumerates — fails naming that fixture. `make check` green.

The item is now backed by a measured number instead of a grep, and I noted the likely cheapest win: those 7 statechart fixtures are a **runner** gap, not a missing feature, since cs already has the state chart and a conformance runner for it.

### Re: do [#verifycrdtplaneruntimein] — opus-5

Audited, and the audit came back clean — which turned out not to be the interesting part.

**All seven bindings count correctly.** rs, py, js, go, dart and zig each increment on log insertion, before any winner logic. go and dart go further and track `changed` as a value separate from `applied`, which is exactly the distinction cpp collapsed; py states the rule outright in its docstring. And **lazily-cs has no CRDT plane at all**, so there was nothing there to audit — worth saying plainly, because "the 7 bindings" implied seven implementations and there are six.

**The real finding is that rs was correct only by inspection.** It replayed *neither* distributed fixture — both sat in its coverage allowlist — and nothing anywhere asserted `applied_count`. A correct implementation with no gate is one refactor away from cpp's bug, and this session has shown repeatedly that "I read it and it looked right" is not a durable state to leave things in. So rs now has the runner.

**The mutation check produced a false negative first, and it explains why rs is immune.** My initial mutation — count only ops that changed the winner — did not fail the test. Not because the test was weak: `apply_remote` *sorts* incoming ops ascending, so in log order every op wins, and the winner-change count and the insertion count coincide. That sort is precisely why rs cannot have cpp's bug. Reproducing the real defect needs the sort removed as well; with both changes the test fails 2 vs 5. Had I stopped at the first mutation I would have concluded the test was useless and deleted it.

**Two things the runner deliberately does not do**, recorded rather than left as a silent gap. It does not assert the fixture's `converged` view — that is the winning op's raw `Inline` bytes per (node, key), and rs's plane is typed-cell oriented, merging serialized CRDT state into registered cells with no accessor for the raw winning payload; a byte-valued `LwwRegister<Vec<u8>>` does not bridge it because `merge_state` deserializes and the cells stay empty. And `crdt_sync_frames.json` remains unreplayed, so it stays in the allowlist. Asserting half a fixture and calling it covered is the marker-only failure this whole line of work exists to prevent, so I filed the remainder instead of quietly claiming it.

**The Makefile bit me twice in the same way the guards have all session.** First the target was referenced by `check` before its recipe existed, so `check` died on "No rule to make target". Before that, the test only ran at all because `test-crdt-plane` happens to run every target under those features — incidental coverage that would vanish the moment someone narrowed that target. It is named explicitly now, and I verified it by breaking the test and confirming `make check` goes red rather than by assuming the dependency line worked.

`make check` green. Pushed as `b31c61d`.

### Re: do [#lazilydistributedconform] — opus-5

> **Queue prompt:**
>
> do [#lazilydistributedconform]


Closed by building the missing accessor rather than by re-recording the limitation.

**`CrdtPlaneRuntime::converged()`** returns the winning op's payload per `(node, key)` — the shape the canonical fixture states convergence in, and the shape lazily-py and lazily-go already expose. rs had only the typed read `value::<C>()`, which structurally cannot answer it: registered cells hold *deserialized* CRDT state, so there was no way to compare against the fixture's raw bytes and the runner skipped that half.

The fold is the op log rather than a second source of truth, and that distinction is the point. `missing_since` already yields every op in ascending stamp order, so writing each into a map keyed by `(node, key)` leaves the greatest stamp per address — and `HlcStamp` orders by `(wall_time, logical, peer)`, so the peer tiebreak the fixture exercises **falls out** instead of being reimplemented inside the accessor where it could drift from the log's own ordering.

**`crdt_sync_frames.json` was a different contract than I assumed.** I expected another plane-behaviour fixture; it is a *wire* fixture — frames carrying a `CrdtSync` payload plus assertions on frontier and op counts, pinning the codec boundary. So it needed its own replay, not an extension of the first. Two details matter there: it deserializes through the real `CrdtSync` type rather than reading the JSON fields, because reading the fields would assert the fixture against itself; and it re-serializes, because a codec that drops ops on the way *out* would satisfy every count checked on the way in.

Before this, I confirmed the allowlist entries were honest rather than an artifact of the static guard. rs's IPC conformance runner loads fixtures by explicit name and never touches `distributed/` — unlike lazily-cs, which enumerates its corpus and therefore *looked* uncovered when it was not. Same guard, opposite conclusion, and only checking the loader distinguishes them.

Mutation-checked both: making `converged()` keep the first op per address instead of the greatest stamp fails the LWW scenario; clearing ops after deserialization fails the frame `op_count`.

Both entries are out of the coverage allowlist — rs now genuinely replays the whole distributed corpus. `make check` green, pushed as `423ac46`.

### Re: do [#lazilyupgradeconformance] — go, and what it found — opus-5

go is upgraded, and unlike js, py and cs it did **not** come back clean — it found real drift the static guard is structurally blind to.

**The seam had to be introduced, not found.** Go has no interception hook, `strace` is unavailable here, and the package had no shared loader — 24 read sites across 22 files, each with its own per-file helper. So `specReadFile` replaces `os.ReadFile` throughout the test package. Every test file is `package lazily`, so one helper serves all of them and the substitution is a single token; reads outside the corpus pass through unrecorded, which makes routing everything through it harmless.

**It found 8 fixtures named in the sources but never opened**, in two classes, neither visible to a grep.

Two were reading **vendored copies** under `test/conformance/` rather than the canonical sibling. They are byte-identical today, so nothing was actually wrong — but the guard was counting the canonical fixture as covered on the strength of a filename appearing in a path string that pointed somewhere else. That is precisely the hazard lazily-cpp's fixture loader warns about, where a vendored fixture silently shrank to a third of its source. Both now read the canonical corpus, and the opened count rose 107 → 109.

The other six appear only in the reactive-graph runner's **known-divergence ledger**, with honest reasons like "no merge/fold node kind in this runner". Nothing was concealed — but a skip entry is not a replay, and a source grep cannot tell those apart. The allowlist is reseeded from runtime evidence and now reads 15 uncovered rather than the 9 a grep reported. The number got worse because it got true.

**One trap, same family as the cs one.** The manifest path must be absolute: `go test ./...` runs one binary per package *from that package's directory*, so a relative path scatters partial manifests instead of accumulating a union.

Mutation-checked: removing the manifest fails as missing evidence, and dropping one entry — a runner that stops opening a fixture it still names — fails naming that fixture, which is exactly the case the static guard could not see.

I did **not** delete the now-orphaned vendored copies, since nothing reads them and deleting files was not what this item asked for. I corrected the stale comment that still pointed at them and filed the decision: delete, or add a diff-against-canonical check so drift fails the build. Leaving a vendored copy that nothing reads is the trap that created this finding in the first place.

Four of seven done (js, py, cs, go). rs, dart and zig remain, all with go's shape, and the item now carries the recipe and the effort estimate rather than just the goal.

### Re: do [#lazilyvendoredfixture] — opus-5

> **Queue prompt:**
>
> do [#lazilyvendoredfixture]


Resolved the second way the item offered, because my own framing of it was wrong.

I filed that item saying the vendored copies were "read by NOTHING" after I repointed two direct reads at the canonical sibling. That was incorrect. `loadConformanceFixture` deliberately tries the lazily-spec sibling *first* and falls back to `test/conformance/` — the copies let the suite run offline. Deleting them, which is what I half-recommended, would have removed a working feature to satisfy a bookkeeping complaint. What I had actually fixed earlier were two reads that bypassed that fallback order and went straight to the vendored copy.

So the copies stay and drift becomes a build failure. When the canonical corpus is present, every vendored file must be byte-identical to its counterpart. All 7 match today — that is now enforced rather than a coincidence nobody was checking.

**Why the runtime manifest does not already cover this**, which is the part worth keeping: the manifest records that a file was **opened**, not that it was the **right** file. The two direct reads were opening the vendored copy, and both the source grep and the runtime manifest were perfectly satisfied. Three layers of coverage checking, and none of them could see that the bytes came from the wrong tree.

It also catches a case I had not considered until writing it: a vendored file whose canonical counterpart was removed or renamed upstream, leaving the copy as the only surviving version of a fixture the spec no longer has.

Absence of the canonical corpus **skips** rather than passes. With nothing to compare against, a pass would assert something it never checked — and the offline case is exactly when the copies are load-bearing and least verifiable. A walk that compares zero files is a hard failure, not a quiet success.

Mutation-checked three ways: a drifted copy fails and names the byte counts (1118 vs 1280), a vendored file with no canonical counterpart fails, and an empty walk fails as vacuous. I also confirmed `make check` genuinely runs it by drifting a fixture and checking for exit 2 — `t.Logf` output is invisible without `-v`, so the green run I got first proved nothing about whether the gate had executed at all.

Pushed as `08c8b00`.

### Re: do [#lazilyconformancecoverag] — opus-5

I set out to take the cheap win I had flagged on this item, and found there is no such win — the note I left was wrong, and wrong in a way that would have sent the next reader down a dead end.

**The correction.** Last cycle I wrote that the 7 statechart fixtures were "a runner gap rather than a missing feature — likely the cheapest win", because cs has `StateChart.cs` and a statechart conformance runner. I derived that from the **static** grep. cs enumerates its corpus, so no fixture filename ever appears in its sources, and the static count under-reported *every* area. Measured at runtime, cs opens all 7 statechart fixtures. It also fully replays reactive-graph 21/21, materialization 3/3 and familysync 1/1. The "cheapest win" did not exist because the work was already done.

**And the deeper claim was wrong too.** There is no runner-gap win anywhere in cs. All 87 unreplayed fixtures are genuinely missing surfaces, and each is already declared in a per-runner `Unsupported` ledger naming the blocking surface — QueueCell, TopicCell, SeqCrdt, TextCrdt, the keyed MergeCell collection. Nothing is hidden; the list is honest and the work is implementation, not wiring. The item now says so, with the real per-area breakdown.

Worth noting the same static-vs-runtime confusion has now bitten me twice on this binding — first when I predicted the 94-entry allowlist was "largely false" and measurement showed it understated by 7, now here. The static guard is not merely weaker on cs, it is *systematically misleading*, because the loader shape it measures has nothing to do with coverage.

**One measurement error of my own**, caught rather than shipped: my first attempt to read the manifest passed `LAZILY_CONFORMANCE_MANIFEST` on the command line, and the Makefile overrides it with an absolute path — so the file I inspected did not exist and statechart looked like 0 opened. I noticed the file was missing rather than reading the empty result as evidence. Had I not, I would have "confirmed" my wrong hypothesis.

**QueueCell is filed with its design settled**, because the interesting question got answered along the way. cs's `Computed<T>` exposes no per-node invalidate, so I initially thought the memoized-derive reader-kind design was inexpressible here and QueueCell was blocked on a new Context primitive. It is not: cs's own `ReactiveMap` already solves this with `Source<int>` version cells bumped via `.Set()`, which is the family-wide idiom. That is **not** the counter-polling the spec bans — the reader is a `Computed` that reads the version to register the edge and derives the real value from storage, exactly as lazily-cpp's QueueCell does. So no new primitive, and the successor item carries that conclusion rather than making the next person rediscover it.

It also carries the trap: the 5 queue fixtures need their own runner, because `CollectionsConformanceTests` dispatches map ops and cannot execute push/pop/close/batch — removing them from its `Unsupported` ledger would break it rather than extend coverage.

### Re: do [#lazilyshipqueuecell] — opus-5

> **Queue prompt:**
>
> do [#lazilyshipqueuecell]


Shipped. cs opens 42 of 124 now, up from 37 — the first of the 87 to actually move.

The design settled last cycle held up: each reader kind gets its own `Source<int>` version cell, bumped when and only when that kind's derived value changes. Worth restating why that is not the counter-polling the spec bans — a reader is a `Computed` that reads its version cell to register the dependency edge and then derives the real value from storage. Callers never see a version number. It is the same mechanism cs's `ReactiveMap` already uses for membership and order, and since `Computed<T>` exposes no per-node invalidate it is also the only expressible route. No new Context primitive was needed, which is what I had been unsure of.

Bumps run inside one `Batch` so a subscriber cannot observe a partial transition — `len` decremented while `is_full` still reads stale.

**The runner had to be separate, as predicted.** `CollectionsConformanceTests` dispatches map operations and cannot execute push/pop/close/batch; removing the 5 entries from its unsupported ledger would have broken it rather than extended coverage. Those entries now name the runner that does replay them, so both ledgers stay true.

**The assertion that carries the weight is the `invalidates` matrix, not the values.** A queue that woke every reader on every operation produces exactly the right head, length and flags at every step — only the matrix separates independent reader kinds from one shared signal.

**The suite passed on the first run, which is precisely when I trust it least.** Three mutations, all caught: making push always invalidate head fails naming reader-kind independence; making pop *not* invalidate head fails the same assertion from the other side; conflating `Closed` with `Empty` on a drained closed queue fails the closure lifecycle. Without those, "passed first try" would have been indistinguishable from "asserted nothing".

One correction against myself: my positive-proof guard guessed ≥100 invalidation checks and the real count is 85. It failed on the first run for that reason alone — every genuine assertion had passed. I set it to the measured number rather than leaving a threshold that happened to be wrong in the safe direction; a guard that fires on its own arithmetic teaches the next reader to weaken it.

`make check` green, pushed as `01d2cf3`. Remaining in cs: 82 fixtures, all genuinely missing surfaces — TopicCell (4) and WorkQueueCell (2) are the next coherent slices in the same row.

### Re: do [#lazilycppsignalingroom] — opus-5

Fixed and pinned. `SignalingRoom::forward` now reports non-delivery and the dispatch answers the sender with `unknown_target`, on the same channel `not_joined` and `permission_denied` already use.

**Why this survived is the interesting part.** The fixture was listed as *deliberately absent* precisely **because** the library was known-wrong — "a faithful runner is red until this is fixed". That reasoning is backwards in a way worth naming: the fixture that would have caught the bug was excluded on the grounds that it would catch the bug. The gap was documented honestly and that documentation is exactly what kept it open. The runner lands with the fix, so the reason no longer justifies its own absence.

**The invariant it pins is anti-spoof**, not routing. A client sends a directed frame carrying `to`; the server must forward it with `from` set to the **sender's server-registered** peer id, never a client-supplied value. A room that echoed a client-supplied `from` produces the right frame types, the right routing and the right roster at every step — only comparing `from` against the registered id catches it. Roster checks (excludes self, ascending) come along for free.

**Writing the runner exposed a second defect neither of us was looking for.** `signaling.hpp` calls `std::sort` without including `<algorithm>`, and compiled only because every existing translation unit happened to pull it in first. No test had ever included that header on its own — the absence of a runner was hiding a header that could not stand alone. Fixed.

`frames.json` stays declared-absent: it needs signaling wire serde this binding does not have. I kept the reason rather than quietly improving the count, since asserting one of two fixtures and calling the area covered is the marker-only failure this whole line of work exists to prevent.

Mutation-checked both ways: restoring the original drop fails with "expected an error frame to conn a but nothing was delivered" — the exact defect — and forwarding a client-supplied `from` fails the anti-spoof assertion naming the registered id.

`make check` green, 94 canonical fixtures replayed, up from 93. Pushed as `a0f390e`.
<!-- agent:boundary:81f17509 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-commit-push" priority go -->
- ~~do [#lazilyshipqueuecell]~~
- ~~do [#lazilyvendoredfixture]~~
- ~~do [#lazilyupgradeconformance]~~
- ~~do [#lazilyconformancecoverag]~~
- ~~do [#lazilydistributedconform]~~
- ⏭️ do [#lazilythreadsafetopiccel]
- ⏭️ do [#lazilythreadsafeworkqueu]
- ⏭️ do [#lazilybenchmarkregressio]
- ⏭️ do [#lzqfjs]
- ⏭️ do [#lzqfports]
- ⏭️ do [#lzqfzig]
- ⏭️ do [#maphandlecleardependents]
- ⏭️ do [#lazilycellfamily]
- ⏭️ do [#lazilyspecdisagrees]
- ⏭️ do [#lzjsnpmtrustedpub]
- ⏭️ do [#lzcsqueues]
- ⏭️ do [#lzcscrdt]
- ⏭️ do [#lzcsipc]
- ⏭️ do [#lzcssync]
- ⏭️ do [#lzcstemporal]
- ⏭️ do [#lzcscluster]
- ⏭️ do [#lzcsplane]
- ⏭️ do [#lzcspackage]
- ⏭️ do [#asyncplanenever]
- ⏭️ do [#lazilyrenameinverted]
- ⏭️ do [#readmeexamplesreference]
- ⏭️ do [#signalgetsignallive]
- ⏭️ do [#recommendeddecidewhether]
- ⏭️ do [#lazilycppconformance]
- ~~do [#lazilycppsignalingroom]~~
- do [#lazilycppcollections]
- do [#trackedcopieslazily]
- do [#firstunificationwire]
- do [#prototypeunifiedreactive]
- do [#lazilyphaseextract]
- do [#unifythreemap]
- do [#lazilymapunification]
- do [#zigdefectsfound]
- do [#lazilyasynccomputedmapde]
- do [#lazilytwoimplementations]
- do [#lazilydartcomputedmap]
- do [#recommendedspecdecision]
- do [#lazilyasyncmap]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#lazilyupgradeconformance] lazily: upgrade the conformance-coverage guard from static to runtime in rs, dart, zig. DONE for js, py, cs and go. go was the one that paid off immediately — see below. Remaining three have the same shape as go did: no interception seam, no shared loader, 20-40 read sites spread across per-file helpers. The go recipe is the pattern: introduce ONE package-level `specReadFile`-style helper, substitute the raw read call token across the test files (they share a package/namespace, so one helper serves all), record ids relative to the conformance root, APPEND (several test binaries contribute), flush at exit, and make a MISSING manifest FAIL as missing evidence. The manifest path must be ABSOLUTE — several bindings run test binaries from a per-package working directory. Effort estimate from go: ~1 helper file + a mechanical single-token substitution across ~22 files
- [ ] [#lazilyconformancecoverag] lazily-cs: 87 of 124 canonical fixtures unreplayed (cs opens 37). MEASURED via the runtime manifest 2026-07-26. CORRECTION — an earlier note on this item claimed the 7 statechart fixtures were a cheap "runner gap"; that was derived from the STATIC grep and is WRONG. cs enumerates its corpus, so no fixture filename appears in its sources, and the static count under-reported every area. Runtime truth: cs FULLY replays reactive-graph 21/21, statechart 7/7, materialization 3/3, familysync 1/1. There is NO runner-gap win anywhere in cs — every one of the 87 is a genuinely missing surface, each already declared in a per-runner Unsupported ledger with the blocking surface named. Unreplayed by area: collections 15, lossless-tree 9, root 8, message-passing 8, reliable-sync 8, rateshape 6, coordination 5, resilience/service/temporal/windowing 4 each, presence 3, agent-doc/distributed/signaling 2 each, crdt-tree/membership/receipts 1 each. Work it down against tasks/software/lazily-cs-parity-plan.md by SHIPPING SURFACES, not by wiring runners
- [ ] [#lazilythreadsafetopiccel] lazily-rs: ThreadSafeTopicCell + AsyncTopicCell. Same per-flavor rules as #lazilythreadsafequeuecel: own graph, own reader handles, bumps outside the lock, batch() for multi-root invalidation. Topic Core reader kinds per cell-model.md are subscribe/reconnect/disconnect/publish/read/read_stream/advance. Update the flavor ledger in the SAME change, and make the corpus replay against the new flavor rather than only flipping the flag — plan: tasks/plan-lazily-queue-family-flavors.md
- [ ] [#lazilythreadsafeworkqueu] lazily-rs: ThreadSafeWorkQueueCell + AsyncWorkQueueCell. Core surface is push/claim/ack/nack/reap_expired plus the three independent reader kinds (pending / in-flight / dead-letter). Visibility-timeout redelivery needs a clock seam, which is NOT flavor-specific — reuse the sync one rather than minting a per-flavor clock. Update the flavor ledger in the SAME change and make the corpus replay against the new flavor. KEPT OPEN 2026-07-26: this reached the queue head before its own stated prerequisite existed — it says "same rules as the reference pair", and that pair had not been built. ThreadSafeQueueCell now ships (46d81e4) and IS the pattern to copy: memoized-derive reader kinds on its own graph, Arc<Mutex<S>> storage with invalidation OUTSIDE the lock (holding storage while invalidating inverts lock order against readers and deadlocks), multi-root invalidation via batch(), ledger flag flipped only alongside a real replay, plus an effect-based atomicity test because a step replay cannot see non-atomic invalidation. The THREAD-SAFE half is unblocked now; the async half waits on the AsyncContext prerequisites in #lzqfrs — plan: tasks/plan-lazily-queue-family-flavors.md
- [ ] [#lazilybenchmarkregressio] lazily-rs: the benchmark regression budgets have NO CI enforcement. ci.yml enumerates individual cargo test invocations and never runs `make check` or `benchmark-check`; regressions.yml runs only the loom model. So every budget in BENCHMARKS.md is enforced solely by whoever remembers to run `make benchmark-check` locally with a populated target/criterion — which, until #lazilyfailstarget, was nobody, because the gate hard-failed on a pruned target dir. Add a scheduled job (regressions.yml already has a nightly cron and is the natural home) that runs `cargo bench` then `make benchmark-check-strict`, so a real regression fails somewhere other than one developer machine. Note the numbers are machine-specific, so the job needs a stable runner class or relative-delta budgets rather than absolute ones
- [ ] [#lazilyasyncmap] lazily-rs async map: `observe` is still hard-bound to &AsyncContext (the UNTRACKED surface), so reading a keyed async entry from inside a compute cannot subscribe to it. CONFIRMED as a family-wide class, not an rs quirk: the same defect was found and fixed in go (its SINGLE-THREADED map's Keys/Len/ContainsKey called the zero-argument Source.Get(), documented in core.go as an untracked external read — probed directly, a reader still reported 2 after a third key was added), in cs (async Len read a membership cell that was written on every mutation and never read anywhere), and in kt (async Core reads spelled against the bare AsyncContext register no edge; this graph tracks through AsyncComputeContext). All three now take an explicit read surface. rs is the ONLY binding left where a keyed async entry read cannot subscribe. Fix shape is settled and shipped 8 times over: make the read generic over the read surface so a compute view registers the edge and a bare context does not
- [ ] [#maphandlecleardependents] MapHandle::clear_dependents exists only on the SYNC handle trait (lazily-rs cell_family.rs:134); the ThreadSafe and Async handle traits have no disposal hook. PARTIALLY RESOLVED 2026-07-26 — the blocker claim was wrong and is now disproven in practice. lazily-cpp shipped it: both thread-safe traits dispose through ctx.context() (BasicThreadSafeContext and AsyncContext each project onto a real Context, so no new primitive was needed), and AsyncSlotHandle gained a clear_dependents that drops the cached value since it has no graph dependents to clear. lazily-cs shipped it too, and found the reason it matters: AsyncComputedMap.Remove never disposed the removed node, so a reader holding the handle kept being served the removed entry's last resolved value. REMAINING: rs's own ThreadSafe/Async handle traits still have no hook; py has none on ANY flavor, so its removes drop the orphaned node without teardown consistently across all three. Note cs also needed a TryGetHandle before the disposal was observable enough to test at all
- [ ] [#lazilycellfamily] lazily-rs cell_family.rs:344-345 documents the present set as "only grows (deferral, not de-allocation)" ten lines above a `remove` that shrinks it (:325-334). Pre-existing doc bug, independent of any refactor — fix the comment or scope it to the materialization plane.
- [ ] [#lazilyspecdisagrees] lazily-spec disagrees with itself on the Dart thread-safe row: coverage.json marks the ThreadSafeComputedMap row shipped for Dart, while docs/coverage.md:85-87 declares the thread-safe context "not applicable" for Dart (single isolate, no shared mutable heap). Resolve in the spec — this is the same question as whether a thread-safe flavor is meaningful on a single-threaded runtime.
- [ ] [#lzqfzig] lazily-zig: the queue family is NOT graph-backed — QueueCell holds a *Context but mints no node; reader-kinds are five u64 version counters callers must poll. It scores green on the coverage row anyway. Make it graph-backed before adding flavors, or the flavor work lands green against nothing. Same finding as zig's sync ReactiveMap; consider one pass for both — plan: tasks/plan-lazily-queue-family-flavors.md
- [ ] [#lzqfjs] lazily-js: QueueCell/TopicCell/WorkQueueCell are not reactive at all — src/queue.js takes no context, contains ZERO ctx references, and reports invalidation by diffing a private snapshot. It scores green on the coverage row. Bind the family to a context and mint real reader-kinds, and check whether the collections fixtures are being satisfied by a non-reactive model here the way test/collections.test.js drives src/collections.js instead of the reactive families — plan: tasks/plan-lazily-queue-family-flavors.md
- [ ] [#lzqfports] lazily: Phase 5 — port the queue-family flavors to cpp, dart, py, go, kt (and build cs flavor-complete from the start under #lzcsqueues). One subagent per binding, NEVER two against one repo. Dispatch only after #lzqfspec + #lzqffx + #lzqfrs are committed so each agent has a settled spec, a red per-flavor gate, and a reference diff. cpp additionally needs its Source<uint64_t> version cells migrated if Phase 0 picks memoized-derive. KEPT OPEN 2026-07-26: #lzqfspec and #lzqffx are now committed, but #lzqfrs (the rs reference implementation) is not, so the stated precondition is unmet — dispatching now would be six independent guesses at an unpinned shape. Unblocks the moment #lzqfrs lands — plan: tasks/plan-lazily-queue-family-flavors.md
- [ ] [#firstunificationwire] FIRST, before any unification: wire the collections atomic-move fixtures (cellmap_atomic_move.json, cellmap_independence.json) into every binding replay harness against ALL THREE flavors. Today zig never replays them at all (only doc-comment references at reactive_map.zig:32,:39) and js replays them only against its fourth, non-reactive collections.js model — so the reactive moveTo has no fixture backing anywhere. Without this the unification lands green against nothing. Plan: tasks/plan-lazily-reactivemap-unification.md
- [ ] [#prototypeunifiedreactive] Prototype the unified ReactiveMap shape in lazily-zig and lazily-cpp — NOT lazily-cs. cs ships only 13 of 48 coverage rows (35 absent) and has NO thread-safe or async SourceMap at all; its conformance substitutes a hand-locked sync map (MaterializationConformanceTests.cs:305) and a bare Dictionary (:336). Prototyping a unification against two families that do not exist would invent the surface rather than converge one, and cannot surface what breaks. zig and cpp are feature-complete, can unify all three families, and neither exposes maps on its C ABI. Plan: tasks/plan-lazily-reactivemap-unification.md
- [ ] [#lazilyphaseextract] lazily-rs Phase A: extract the closure-free KeyedOrder<K,H> core (entries + order + membership + present-set + position/keys/len/contains_key + move_to/move_before/move_after) shared by all three families; thin per-model shells keep materialize/observe. Do NOT route through SyncReactiveGraph — its Send+Sync bound is stricter than the concrete Context API and would break Rc-capturing callers. `remove` stays out of the core (monotone present set is fixture- and Lean-backed). Plan: tasks/plan-lazily-reactivemap-unification.md
- [ ] [#unifythreemap] Unify all three map families in lazily-zig and lazily-cpp — both can fold async in, unlike the rest. zig: fn ReactiveMap(comptime Ctx,K,V,kind) with Ctx.Handle/Ctx.Observed/Ctx.Lock projections; the proof already exists in-tree as Engine(comptime Model) at reactive_graph_conformance.zig:1206 over SyncModel/TsModel/AsyncModel. cpp: traits class + existing duck-typed Cx param; only Source::clear_dependents(Context&) and Computed::clear(Context&) block it, and AsyncSlotHandle needs one added clear_dependents. Neither exposes maps on its C ABI. Plan: tasks/plan-lazily-reactivemap-unification.md
- [ ] [#lazilymapunification] lazily-go map unification BEFORE it hits v1 — still v0.22.0 on an unversioned module path, so a breaking change costs nothing now; after v1 it costs a /v2 module path plus every import rewrite. Mechanism: closure fields (already proven in-repo at reactive_map.go:92-96) or one extra type param. Plan: tasks/plan-lazily-reactivemap-unification.md
- [ ] [#zigdefectsfound] zig defects found while surveying, independent of unification: (1) the sync ReactiveMap is NOT graph-backed — stores plain V and fakes reactivity with u64 counters callers must poll, so keys()/len()/contains() register no edge (reactive_map.zig:140-419 touches ctx only for .allocator); (2) ThreadSafeComputedMap mints the WRONG node kind — mintLocked always calls tsctx.cell ignoring entry_kind (thread_safe_reactive_map.zig:98), so its computed entries are cells; (3) moveTo desyncs on allocator failure — orderedRemove then insert-catch-return-false leaves the key in materialized but absent from order (reactive_map.zig:288-303).
- [ ] [#lazilyasynccomputedmapde] lazily-cs AsyncComputedMap defects: Len() is documented as reading through the membership plane but returns _order.Count, and _membership is written at :117 and never read anywhere (dead code, so Len is non-reactive); Remove (:92-98) never disposes the removed AsyncComputed, unlike both sync clear delegates (ReactiveMap.cs:299,:405), leaving readers on a stale cache. Also: no thread-safe or async SourceMap exists at all — conformance papers over it with a hand-locked sync map (MaterializationConformanceTests.cs:305) and a bare Dictionary (:336).
- [ ] [#lazilytwoimplementations] lazily-js: two implementations disagree on moveTo with a negative index — reactive-family.js:270 uses Math.min(index, Math.max(len-1,0)) with no lower clamp, collections.js:196 uses Math.max(0, Math.min(index, len-1)). moveTo(key,-1) splices before the last element in one and at 0 in the other. Untested in both; pick one before any unification silently changes a published surface.
- [ ] [#lazilydartcomputedmap] lazily-dart: ComputedMap entries are Slot, not Computed (collections.dart:430). Post-v2 Slot is the UNGUARDED pass-through while Computed carries the == memo guard — so ComputedMap does not have the guard its name implies, and this leaks publicly through handle(). Decide whether the handle becomes Computed.
- [ ] [#recommendedspecdecision] [recommended] Spec decision for lazily-spec, blocking the family unification: (1) does the atomic-move contract in cell-model.md:275 bind EVERY flavor or only the source map? All nine bindings fail it on thread-safe and async, yet coverage.json marks all three rows green for every binding. (2) Is a thread-safe flavor meaningful on single-threaded runtimes? dart ships a 182-line duplicate family the spec marks N/A, and js README:633-640 argues the opposite of its own source comment at thread-safe.js:134-137.
- [ ] [#lazilycppconformance] lazily-cpp: 10 conformance runners are MARKER-ONLY — they open the canonical fixture but only assert its `"model"` string, then replay hand-transcribed steps (rateshape 6, coordination 5, temporal/windowing/service/resilience 4 each, presence 3, membership/crdt-tree 1 each = 33 fixtures). They pass a did-you-load-fixtures check while being blind to any change in the corpus steps or expectations. Convert to real replays; the coverage guard counts them as covered today.
- [ ] [#lazilycppcollections] lazily-cpp: 15 collections fixtures still uncovered — 9 with no coverage at all (cellmap_atomic_move, cellmap_independence, keyed_reconciliation_lis, semtree_incremental, stableid_alignment, textcrdt_convergence, textcrdt_delta_sync, workqueue_competing_delivery, workqueue_lease_deadletter) and 6 hand-transcribed (mergecell_algebra, 5x queuecell_*). Largest single body of remaining conformance work; spans five separate primitives. Tracked in the KNOWN_UNCOVERED ledger.
- [ ] [#trackedcopieslazily] The tracked copies of lazily-spec conformance fixtures inside lazily-py (tests/conformance), lazily-dart (test/conformance), lazily-rs (tests/conformance) and lazily-go (test/conformance) are kept byte-identical to canonical BY HAND with no sync script or drift check. This cycle had to flip the same fixture in three places. Add a sync/verify script (or make the runners read the sibling checkout) so a canonical edit cannot silently miss a vendored copy.
- [ ] [#asyncplanenever] Async plane never migrated to the v2 kernel: AsyncCellHandle / AsyncSlotHandle / AsyncSlotState survive in cpp, go, js, kt, py, zig, cs. Sharpest case is lazily-cpp `AsyncContext::memo()` (async_context.hpp:144), still live and undeprecated while the sync `Context::memo` is DELETED. Migrate to Async{Source,Computed} with deprecated aliases.
- [ ] [#lazilyrenameinverted] [recommended] lazily-py keeps `Cell`/`CellSlot` as the native class names with `Source = Cell` / `SourceSlot = CellSlot` aliases (cell.py:35,191,219-220). This is NOT drift — it is documented and motivated: cell.py is inside the mypyc compilation unit (setup.py compiles slot/cell/signal/effect/batch as one unit for cross-file native-class inheritance BaseSlot -> CellSlot), and the docstring states Cell "stays the native class name (for mypyc native-struct reads and isinstance stability across the family)". Flipping would rename the native class and change isinstance identity. Do NOT flip without deciding whether mypyc native-struct naming or family-wide vocabulary consistency wins; that is a spec/ownership question, not a cleanup.
- [ ] [#readmeexamplesreference] README examples reference types that no longer exist: lazily-go README:82 `lazily.NewCell` (never defined, example does not compile) and README:209 `*Cell[V]`; lazily-cpp README:54,280,387 + AGENTS.md:43 `CellHandle`/`SlotHandle` (deleted); lazily-dart README:38,329 claims a `Signal` back-compat alias that does not exist; lazily-py README:225 calls `CellSlot` canonical.
- [ ] [#signalgetsignallive] signal/getSignal still live and UNDEPRECATED on the threadsafe/async planes after the sync plane retired them: lazily-kt ThreadSafeContext.kt:195,247 + AsyncContext.kt:624, lazily-js reactive-async.d.ts:85, lazily-zig root.zig:86,90. Deprecate or migrate to eager Computed.
- [ ] [#recommendeddecidewhether] [recommended] Decide whether the domain *Cell family (QueueCell / TopicCell / RelayCell / MergeCell / TimerCell / LeaseCell / CircuitBreakerCell / PresenceCell / ...) follows the kernel to *Source, or is deliberately pinned as spec-level naming. Three independent bindings flagged it as the largest remaining Cell-vocabulary surface. It must move in lockstep across all 9 bindings, so it needs a decision before anyone starts. RECOMMENDATION 2026-07-26 (agent): KEEP *Cell. The kernel renamed the two value-node KINDS (Source/Computed) and kept Cell as the concept for a value-bearing reactive node. A QueueCell is not a kind — it composes them (five Source<u64> reader-version cells plus derived readers), so QueueSource would imply "written from outside, folds under a merge policy", which is false for a queue. Same for TimerCell/LeaseCell/CircuitBreakerCell/PresenceCell: each is a composite of a pure core plus projecting cells. KEPT OPEN: naming must converge with the operator before editing (9 bindings in lockstep + deprecation surface on every public type). Unblocks on a yes/no. If YES, scope is: spec cell-model.md first, then 9 bindings in lockstep with deprecated aliases, plus a decision on whether MergeCell collapses entirely (under the kernel it is already Source<T,M> with a non-default policy)
- [ ] [#lzjsnpmtrustedpub] lazily-js: register @lazily-hub/lazily-js as a trusted publisher on npmjs.com so release.yml can publish unattended. The workflow is correct — its OIDC exchange succeeds and provenance is signed and logged to Sigstore — but the upload is rejected with E404 PUT https://registry.npmjs.org/@lazily-hub%2flazily-js, meaning npm does not associate the package with that publisher (or the token lacks scope permission). This has now E404'd on two consecutive releases and each time required a manual 'npm publish --access public --otp=<code>' from the operator. Fix is npmjs.com-side configuration, not a code change
- [ ] [#lzcsqueues] lazily-cs: QueueCell + QueueStorage, TopicCell, WorkQueueCell, RelayCell with BackpressurePolicy/SpillStore/Transport — plan: tasks/software/lazily-cs-parity-plan.md (rows 12-14, 16)
- [ ] [#lzcscrdt] lazily-cs: TextCrdt + delta sync, SeqCrdt, CrdtTree, LosslessTreeCrdt incl. dotted-frontier anti-entropy and concurrent-merge convergence, LWW/MV registers, PnCounter, CellCrdt; replay crdt-tree + lossless-tree corpora — plan: tasks/software/lazily-cs-parity-plan.md (rows 17-24)
- [ ] [#lzcsipc] lazily-cs: Snapshot/Delta/CrdtSync wire types validated against the lazily-spec JSON Schemas, ShmBlobArena, BlobBackend zero-copy transport — plan: tasks/software/lazily-cs-parity-plan.md (rows 25-27)
- [ ] [#lzcssync] lazily-cs: CrdtPlaneRuntime + anti-entropy, resync coordinator, durable OutboxStore, IpcSink/IpcSource + full-duplex SyncDriver; replay reliable-sync + distributed + message-passing corpora — plan: tasks/software/lazily-cs-parity-plan.md (rows 28-32)
- [ ] [#lzcstemporal] lazily-cs: TimerCell/IntervalCell/CronCell/DeadlineCell over a logical clock, debounce/throttle/sample/probabilistic-sample, tumbling/sliding/session windows; replay temporal + rateshape + windowing corpora — plan: tasks/software/lazily-cs-parity-plan.md (rows 40, 41, 45)
- [ ] [#lzcscluster] lazily-cs: MembershipCell (SWIM + Phi-accrual), LeaseCell/LeaderCell/LockCell/SemaphoreCell/BarrierCell, presence/awareness/ephemeral, circuit breaker/retry/bulkhead/timeout, health/readiness/discovery/registry; replay membership + coordination + presence + resilience + service corpora — plan: tasks/software/lazily-cs-parity-plan.md (rows 42-44, 46-47)
- [ ] [#lzcsplane] lazily-cs: state projection/mirror, CausalReceipts, command-plane-v1, C-ABI FFI boundary, PeerPermissions/RemoteOp, SessionHandshake, instrumentation + benchmarks; replay receipts + signaling + agent-doc corpora — plan: tasks/software/lazily-cs-parity-plan.md (rows 33-39)
- [ ] [#lzcspackage] lazily-cs: multi-target netstandard2.1 / net8.0 alongside net10.0 (INumber<T> in the merge algebra needs a shim or policy split), add a release workflow, publish Lazily to NuGet — needs a NuGet API key from the operator
<!-- /agent:backlog -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/lazily.done.md -->

<!-- completed work archived in tasks/software/lazily.done.md -->
<!-- /agent:done -->
