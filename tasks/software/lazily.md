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
<!-- agent:boundary:42662c0e -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-commit-push" priority go -->
- do [#lazilyfailstarget]
- ~~do [#lzqfcppfx]~~
- ~~do [#lzqfprep]~~
- ~~do [#lzdonequoteattr]~~
- ~~do [#lzqfspec]~~
- ~~do [#lzqfformal]~~
- ~~do [#lzqffx]~~
- ⏭️ do [#lzqfrs]
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
- ~~⏭️ do [#observerwordingcontradic]~~
- ~~⏭️ do [#renametreesource]~~
- 🚧 do [#recommendeddecidewhether]
- do [#lazilycppconformance]
- do [#portconformancecoverage]
- do [#verifycrdtplaneruntimein]
- do [#lazilycppsignalingroom]
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
- ~~do [#portmembershiporder]~~
- do [#lazilyasyncmap]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#lazilyfailstarget] lazily-rs: `make check` fails at `benchmark-check` because target/criterion no longer exists ("no Criterion estimates found under target/criterion; run without --no-run"). Reproduced on a clean stashed tree, so it is environmental, not a code regression — but it means make check cannot currently reach green in this checkout, which makes it useless as a gate. Either run the benchmark suite once to repopulate target/criterion, or make benchmark-check skip (loudly) when no criterion output exists instead of hard-failing, so a pruned target dir does not read as a red suite. Do NOT let it silently pass
- [ ] [#lazilyasyncmap] lazily-rs async map: `observe` is still hard-bound to &AsyncContext (the UNTRACKED surface), so reading a keyed async entry from inside a compute cannot subscribe to it. CONFIRMED as a family-wide class, not an rs quirk: the same defect was found and fixed in go (its SINGLE-THREADED map's Keys/Len/ContainsKey called the zero-argument Source.Get(), documented in core.go as an untracked external read — probed directly, a reader still reported 2 after a third key was added), in cs (async Len read a membership cell that was written on every mutation and never read anywhere), and in kt (async Core reads spelled against the bare AsyncContext register no edge; this graph tracks through AsyncComputeContext). All three now take an explicit read surface. rs is the ONLY binding left where a keyed async entry read cannot subscribe. Fix shape is settled and shipped 8 times over: make the read generic over the read surface so a compute view registers the edge and a bare context does not
- [ ] [#maphandlecleardependents] MapHandle::clear_dependents exists only on the SYNC handle trait (lazily-rs cell_family.rs:134); the ThreadSafe and Async handle traits have no disposal hook. PARTIALLY RESOLVED 2026-07-26 — the blocker claim was wrong and is now disproven in practice. lazily-cpp shipped it: both thread-safe traits dispose through ctx.context() (BasicThreadSafeContext and AsyncContext each project onto a real Context, so no new primitive was needed), and AsyncSlotHandle gained a clear_dependents that drops the cached value since it has no graph dependents to clear. lazily-cs shipped it too, and found the reason it matters: AsyncComputedMap.Remove never disposed the removed node, so a reader holding the handle kept being served the removed entry's last resolved value. REMAINING: rs's own ThreadSafe/Async handle traits still have no hook; py has none on ANY flavor, so its removes drop the orphaned node without teardown consistently across all three. Note cs also needed a TryGetHandle before the disposal was observable enough to test at all
- [ ] [#lazilycellfamily] lazily-rs cell_family.rs:344-345 documents the present set as "only grows (deferral, not de-allocation)" ten lines above a `remove` that shrinks it (:325-334). Pre-existing doc bug, independent of any refactor — fix the comment or scope it to the materialization plane.
- [ ] [#lazilyspecdisagrees] lazily-spec disagrees with itself on the Dart thread-safe row: coverage.json marks the ThreadSafeComputedMap row shipped for Dart, while docs/coverage.md:85-87 declares the thread-safe context "not applicable" for Dart (single isolate, no shared mutable heap). Resolve in the spec — this is the same question as whether a thread-safe flavor is meaningful on a single-threaded runtime.
- [ ] [#lzqfrs] lazily-rs: Phase 3 of the queue-family plan — context prerequisites + reference implementation. ThreadSafeContext needs a multi-root clear_slots equivalent (it has only single-handle clear at thread_safe.rs:3549 — four handles is four frontier walks; verify whether batch() already closes this, do not assume). AsyncContext needs a synchronous-compute computed and a multi-root clear (it has neither; only computed_async at async_context.rs:413). Then ship ThreadSafe/Async QueueCell + TopicCell + WorkQueueCell, each minting its own reader-kind handles on its own graph, Arc<Mutex<S>> storage with invalidation run outside the lock. KEPT OPEN DELIBERATELY: this is Phase 3 and is gated on Phase 1 (#lzqfformal) and Phase 2 (#lzqffx) — the plan requires the per-flavor fixture gate to land BEFORE implementation so the gap goes red first. A drain that reaches this id before those two is out of order, not ready work — plan: tasks/plan-lazily-queue-family-flavors.md
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
- [ ] [#portconformancecoverage] Port a conformance-coverage guard to the 7 bindings that lack one. Only lazily-kt (scripts/check-conformance-coverage.sh) and now lazily-cpp fail the build when a canonical fixture is not actually replayed. rs, py, js, go, dart, zig, cs have no such guard, so the same marker-only / hand-transcribed drift found in cpp is undetected there. Audit each the same three ways: real replay vs marker-only vs absent.
- [ ] [#verifycrdtplaneruntimein] Verify CrdtPlaneRuntime ingest-op counting in the 7 bindings not yet checked. lazily-cpp returned the count of ops that CHANGED THE WINNER rather than ops accepted into the log (4 vs 5 on lww_last_writer_wins), breaking the spec rule that redelivery applies 0 new ops. lazily-kt (CrdtPlane.kt:148) is correct and cpp is now fixed; rs, py, js, go, dart, zig, cs are unaudited for the same divergence.
- [ ] [#lazilycppsignalingroom] lazily-cpp SignalingRoom::forward (include/lazily/signaling.hpp:203) silently drops frames addressed to an unknown peer, but conformance/signaling/anti_spoof_session.json requires an `unknown_target` error frame back to the sender. A faithful signaling runner stays red until this is fixed. The second signaling fixture (frames.json) additionally needs signaling wire serde, which lazily-cpp does not have.
- [ ] [#lazilycppcollections] lazily-cpp: 15 collections fixtures still uncovered — 9 with no coverage at all (cellmap_atomic_move, cellmap_independence, keyed_reconciliation_lis, semtree_incremental, stableid_alignment, textcrdt_convergence, textcrdt_delta_sync, workqueue_competing_delivery, workqueue_lease_deadletter) and 6 hand-transcribed (mergecell_algebra, 5x queuecell_*). Largest single body of remaining conformance work; spans five separate primitives. Tracked in the KNOWN_UNCOVERED ledger.
- [ ] [#trackedcopieslazily] The tracked copies of lazily-spec conformance fixtures inside lazily-py (tests/conformance), lazily-dart (test/conformance), lazily-rs (tests/conformance) and lazily-go (test/conformance) are kept byte-identical to canonical BY HAND with no sync script or drift check. This cycle had to flip the same fixture in three places. Add a sync/verify script (or make the runners read the sibling checkout) so a canonical edit cannot silently miss a vendored copy.
- [ ] [#asyncplanenever] Async plane never migrated to the v2 kernel: AsyncCellHandle / AsyncSlotHandle / AsyncSlotState survive in cpp, go, js, kt, py, zig, cs. Sharpest case is lazily-cpp `AsyncContext::memo()` (async_context.hpp:144), still live and undeprecated while the sync `Context::memo` is DELETED. Migrate to Async{Source,Computed} with deprecated aliases.
- [ ] [#lazilyrenameinverted] [recommended] lazily-py keeps `Cell`/`CellSlot` as the native class names with `Source = Cell` / `SourceSlot = CellSlot` aliases (cell.py:35,191,219-220). This is NOT drift — it is documented and motivated: cell.py is inside the mypyc compilation unit (setup.py compiles slot/cell/signal/effect/batch as one unit for cross-file native-class inheritance BaseSlot -> CellSlot), and the docstring states Cell "stays the native class name (for mypyc native-struct reads and isinstance stability across the family)". Flipping would rename the native class and change isinstance identity. Do NOT flip without deciding whether mypyc native-struct naming or family-wide vocabulary consistency wins; that is a spec/ownership question, not a cleanup.
- [ ] [#readmeexamplesreference] README examples reference types that no longer exist: lazily-go README:82 `lazily.NewCell` (never defined, example does not compile) and README:209 `*Cell[V]`; lazily-cpp README:54,280,387 + AGENTS.md:43 `CellHandle`/`SlotHandle` (deleted); lazily-dart README:38,329 claims a `Signal` back-compat alias that does not exist; lazily-py README:225 calls `CellSlot` canonical.
- [ ] [#signalgetsignallive] signal/getSignal still live and UNDEPRECATED on the threadsafe/async planes after the sync plane retired them: lazily-kt ThreadSafeContext.kt:195,247 + AsyncContext.kt:624, lazily-js reactive-async.d.ts:85, lazily-zig root.zig:86,90. Deprecate or migrate to eager Computed.
- [ ] 🚧 [#recommendeddecidewhether] [recommended] Decide whether the domain *Cell family (QueueCell / TopicCell / RelayCell / MergeCell / TimerCell / LeaseCell / CircuitBreakerCell / PresenceCell / ...) follows the kernel to *Source, or is deliberately pinned as spec-level naming. Three independent bindings flagged it as the largest remaining Cell-vocabulary surface. It must move in lockstep across all 9 bindings, so it needs a decision before anyone starts. RECOMMENDATION 2026-07-26 (agent): KEEP *Cell. The kernel renamed the two value-node KINDS (Source/Computed) and kept Cell as the concept for a value-bearing reactive node. A QueueCell is not a kind — it composes them (five Source<u64> reader-version cells plus derived readers), so QueueSource would imply "written from outside, folds under a merge policy", which is false for a queue. Same for TimerCell/LeaseCell/CircuitBreakerCell/PresenceCell: each is a composite of a pure core plus projecting cells. KEPT OPEN: naming must converge with the operator before editing (9 bindings in lockstep + deprecation surface on every public type). Unblocks on a yes/no. If YES, scope is: spec cell-model.md first, then 9 bindings in lockstep with deprecated aliases, plus a decision on whether MergeCell collapses entirely (under the kernel it is already Source<T,M> with a non-default policy)
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
