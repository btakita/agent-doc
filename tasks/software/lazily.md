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

gated: npm security-key user presence required
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/44ae7f3a5c03b112ffe17aa5c85d000e3d79a6e9a4875e16442309ebf1c06a1c-20260730-231419.md`*

Compacted content:
- Archived 7 response topic(s): do [#lzconsumednotasserted]; do [#lzscenariocoverage]; Protobufs for the key-validation work; 4 more
- Prior summary/context: prior compacted content: Archived 9 response topic(s): do [#designimplementtransport]; do [#lzingressbindings] using ...

### Re: do [#lzbenchbudgetheadroom] — opus-5

> **Queue prompt:**
>
> do [#lzbenchbudgetheadroom]


Measured before changing anything. The two flakes were the visible edge of a much larger problem, and the gate had its tradeoff exactly backwards.

**The sweep.** 750 runs of `examples/instrumentation_profile.rs` across four environments: idle (200), moderate load (200, 8 CPU burners), heavy load (200, 24 burners on 32 cores), and 2-core-pinned via `taskset -c 0,1` (150) — the shape of the GitHub runner the gate actually runs on. Under load **15 of 45 gated counters blew their budget**, some on 25-30% of runs. `thread_safe_contention_batched_write_bursts_16 / set_cell_invalidation`, budgeted at 16, ranges **1 to 256**: under contention invalidation stops coalescing and every write takes its own lock. A budget of 16 over that counter is a coin flip.

**The gate was loose where the signal is and tight where the noise is.** 22 counters have *zero* spread across all 750 runs, and each held the identical constant on 2 cores and on 32 — they count work items, not interleavings. Those were the loosely-budgeted ones: `queue_coalescing / dependency_edge <= 1600` for a counter that is always 64, `cleanup_execution / dependency_edge <= 700` for one that is always 32. A 25x regression passed. Meanwhile the pure-noise counters were pinned at 0.4% headroom.

**The fix — ceilings derived from the recorded spread, never typed.** Every counter carries an `ObservedSpread(samples, min, max)` and its ceiling comes from a rule, not a keyboard:

| Spread | Class | Ceiling | Count |
| --- | --- | --- | ---: |
| zero across all four environments | `deterministic` | the observed value, **exactly** | 22 |
| at most half the observed maximum | `scheduling_sensitive` | observed max + one full observed range | 10 |
| more than half the observed maximum | `scheduling_dominated` | **none** — recorded, reported, not enforced | 19 |

The third row is the honest part. No ceiling over a 1-to-256 counter can distinguish a regression from a busy machine, so gating it is theatre that costs real trust. Those counters stay recorded and printed, and `benchmark-check` now ends with `32 counter(s) enforced; 19 scheduling-dominated counter(s) recorded as observations only and NOT enforced` — a green run can no longer be misread as full coverage.

`make benchmark-spread` (new) reruns the profile N times and prints the measured table, the derived ceilings, a paste-ready `REGRESSION_BUDGETS` block, and any counter that fell outside its recording. Refreshing the numbers is now a measurement, not a guess — and the docs say to take it under load and pinned, since a sweep on a quiet machine is how 0.4% headroom got written in the first place.

**Verification.** Six new tests drive the real gate with synthesized profiles: baseline-green, exact enforcement, proportional headroom as a real boundary in both directions, the deliberate non-enforcement, the coverage report, and "no hand-typed ceilings survive in the source". All five mutation probes confirmed each test goes red when its claimed behaviour is broken — including the anchor that silently failed to apply on the first pass, which I re-ran rather than counting.

`make check` green (721 test-result blocks, 0 failures). Pushed as `48cd26b`; **CI fully green**.

One confirmation worth recording: the CI run immediately before this commit was **red for exactly this bug** — `thread_safe_effect_contention_cleanup_execution_16: other lock_acquisitions 494 > budget 450` on a real runner. That counter measures 332-1572 in the sweep, so it is now classified scheduling-dominated and no longer reddens. Three independent sightings, one cause.

### Re: do [#lzinteroppeerci] — opus-5

Audited all nine bindings, target by target. The headline is worse than the item's claim, and the wiring immediately caught a live break.

**The audit trap first.** A grep for `make check` in the workflows reports all nine bindings as fully covered. Every one of those hits is a **comment** — prose left behind by #lzguardsnotinci explaining what `make check` does locally. No binding runs `make check` in CI; they all run individual steps. An audit that trusted that grep would have concluded there was nothing to do.

**The real gap is universal.** The interop peer self-check — the cross-binding wire-compatibility gate — ran in **zero of nine** bindings' CI, while `make check` runs it in **nine of nine**. The one check that proves nine bindings still agree on a byte layout was the one check no runner ever executed. The item flagged go, js and dart; it is all of them, including lazily-rs.

**Other real gaps found:** `lazily-cs` never ran `check-package.sh` or `check-ffi.sh` in CI; `lazily-dart` never ran the stdlib browser compile (the check that catches `dart:io`/VM-only dependencies leaking into the portable stdlib).

**Claims that did not survive verification** — checked rather than assumed: lazily-go's CI already runs gofmt, so `fmt-check` is not a gap; kt reaches both Lean models through a separate job, and dart reaches `formal-check` through `dart test`; lazily-py's CI runs `poe precommit`, which already covers format/lint/type-check/coverage. On zig the item's concern runs the other way — CI already runs the full `0.15.2 / 0.16.0 / master` matrix, so the pinned toolchains are enforced where it matters.

**The wiring paid for itself on the first run.** lazily-zig went red immediately: `src/interop_peer.zig:410: error: root source file struct 'process' has no member named 'Init'`. The peer had **stopped compiling on the pinned 0.15.2 entirely** — `std.process.Init` and `std.Io` are 0.16+ only, and the peer is the sole file in the tree touching either. It ran from `make check` on one developer toolchain, so the other two never built it. That is precisely the rot the item describes, sitting in the tree, found within minutes of the gate existing.

Fixed by splitting the entry point per toolchain and selecting it in `build.zig` — the same shape `build.zig` already uses for `cell_0_16_test.zig`. The protocol and self-check stay in one shared module; only the process/IO plumbing differs. Verified `interop-peer-check` passes on **0.15.2, 0.16.0 and master**, and zig's `make check` is green.

**Verification.** Every added command was run locally first. All nine repos pushed; **CI green on all nine** (`lazily-rs`, `go`, `js`, `py`, `kt`, `cs`, `cpp`, `dart`, `zig`).

Filed `#lzcheckcireachguard` at the top of the backlog: this audit was done by hand and nothing stops the drift reopening. A guard that resolves each `check` dependency against the workflow's real run steps — comments stripped, with an explicit excuse list for deliberately-local targets — is what would catch the next one.

### Re: #lzcheckcireachguard — the guard is in all nine, and the excuse lists all came back empty — opus-5

Landed `scripts/check-ci-reach.sh` in every binding, byte-identical (sha256 `ba972679…`), wired into each `check` and into each push/PR workflow. All nine are green on their current HEAD.

**How it decides.** It reads each target's recipe through `make -n`, so make variables are already expanded and it compares real command lines rather than source text. Deliberately *not* `make -p`: that dumps the entire environment to stdout, which in a CI log means every secret in the job's env. Each command reduces to an anchor — program basename plus subcommands and flag *names* — and a target counts as reached only when **every** one of its anchors is an in-order subsequence of some CI `run:` body. Keeping flag names is what makes it falsifiable: `go test -race` does not match a step that only runs `go test -count=1`, so deleting the race job reddens the guard instead of being absorbed by the plain test job.

Only `run:` bodies count, and comment lines inside them are stripped first. That is the whole reason this is a script rather than a convention — the original hand audit grepped the workflows for `make check`, got hits in all nine, and every one of those hits was a comment.

**Mutation-checked, not assumed.** 13 mutations against a scratch copy, each restored between runs: deleting the race / interop-peer / coverage step; commenting a step out instead of deleting it; dropping `-race` while `go test` stays; dropping `-count`; a new `check` dependency CI never runs; a half-covered target; a stale excuse; a reasonless excuse; a missing workflow; an empty `run:` haystack; and a `check` with no gate-bearing prerequisite. All reddened. Every binding repeated an independent subset.

**The excuse lists are all empty**, in all nine, and that is the honest outcome rather than a convenience — the agents fixed CI instead of excusing it. What they found:

| binding | what was actually unreachable |
|---|---|
| kt, zig | `lake build` ran only *inside* `leanprover/lean-action`, never in a `run:` body — invisible to the guard and to a reader diffing the workflow. Both now run it explicitly. |
| py | CI ran one opaque `uv run poe precommit`. Four gates were buried inside it; `ruff format` was **absent entirely** and `ruff check --fix` was repairing the tree it judged. Now unrolled into named steps with `--check` / `--no-fix`. |
| dart | `formal-check` reached Lean only indirectly. Adding the step exposed a pre-existing false green: `lake` was never on PATH in that job, so it had been printing `SKIP` and passing on every run. |
| zig | CI passed the manifest via env while `make` passed `-Dconformance-manifest` — same knob, two spellings, which is exactly how gates drift apart. |
| cpp, cs, js, rs, go | already reached; splitting js's monolithic `check:` into named targets is what made that provable per gate instead of one opaque verdict. |

**One bug in my own script, caught by kt and fixed across all nine.** `anchors()` stripped quotes *before* splitting on `;`, so a semicolon inside a quoted message invented a command and reported a phantom unreachable gate — a false **red**, which is the expensive direction. Now split with a quote-state scan first. Re-verified: a recipe carrying `echo "missing build; clone ..."` plus a real `go vet` is reported reached on its real anchor, and the full battery still reddens.

### Three things you should see

**1. `make -p` printed a secret into this terminal.** While prototyping I ran `make -p -n` and it dumped the environment, including `CONTABO_PASSWORD` in cleartext. That is in this session's scrollback and in any recording of it. Rotate that credential, and trim the segment if this was captured. The guard never uses `make -p` for exactly this reason, and the script says so in its header.

**2. lazily-go has a real lost-wakeup bug — filed as `#lzgoasynclostwake`, top of backlog.** CI went red on `TestAsyncEffectRerunsAcrossTwoWrites`. It reproduces locally *without* `-race`, and a 300-iteration probe measured **never-woke=3, slow=0** — the effect never reruns even given 8 seconds, so this is not a tight deadline. `runEffect` clears the dependency set on the loop, then re-registers it from a separate goroutine; a `Set` landing in that gap reaches no dependent, and the effect stays subscribed holding the pre-write value forever. I did not patch it: it is a reactive-kernel change, the same off-loop re-registration exists in every binding's async shell, and a one-binding hotfix is the wrong shape for it.

I reran go's failed job to get CI coverage of the corrected script, and the guard step passed. To be explicit: **that green does not mean the race is fixed.** It fires at roughly 1% per write, so it will come back.

**3. Your queue preset is corrupted.** The header now reads `preset="#sp****ec-test-commit-push …"`. There is no granular `agent-doc write` flag for a queue attribute, so I left your edit alone rather than rewriting it — but until it reads `#spec-test-commit-push` again the preset resolves to nothing.

Also filed: `#lzruffautofixvacuity` (a formatter that rewrites cannot fail, and `make lint` still auto-repairs locally) and `#lzcireachvaranchor` (the guard can't match a make-side literal against a CI-side `$VAR`; fail-closed, worth a family-level decision). One loose end I could not close: lazily-kt's `AGENTS.md` `## Commands` block is stale, but the file carries another session's uncommitted edit and committing it would have dragged that in.

**Correcting my own report from last cycle.** I told you lazily-go had a lost wakeup where the effect "stays subscribed holding the pre-write value forever." That was wrong. I measured it this time instead of inferring it from a failed assertion.

A 400-iteration probe under `-race` that records what the effect *actually holds* when the rerun goes missing: **coalesced-but-correct = 4, truly-stale = 0.** Every single case had the effect already holding the written value. Not one instance of a stale value.

**Why it cannot be stale.** `trackDep` and the value read happen inside one `c.do()` closure on the owner goroutine, so registration and read are atomic. Either the write lands first and the body reads the *new* value with the edge registered in the same turn, or it lands after and the edge is already there to schedule a rerun. There is no interleaving that yields a registered edge next to a stale value. What I mistook for a lost wakeup was two logical runs collapsing into one that observes the final value — the same thing `Batch` does on purpose.

**What was actually broken was the test.** It bumped its run counter at the *top* of the body, so the wait for the first run returned as soon as the body *started*, before `TrackSource` had subscribed. A write landing in that gap gets absorbed into the run already in flight, and the count never reaches the next number. The test asserted an exact run count across a boundary where coalescing is legal.

Fixed by bumping the counter **after** the tracking read, so a run count of N is evidence the edge exists and every later write must schedule a real rerun. Same reorder in `TestAsyncEffectDownstreamOfSlotCascade`.

**Verified and mutation-checked**, because a reorder that makes a flaky test pass is exactly the change that could have defanged it:

- 400 iterations of both tests under `-race`: clean. Previously 1-2% red.
- Suppress the effect's edge registration entirely, and it reddens at the first rerun.
- Stop re-registering *after* the first rerun — the precise "deaf after one rerun" defect these tests are named for — and it reddens at the third wait.

`make check` exit 0, CI green.

**Cross-binding check: lazily-go was the only one.** zig and dart both reach quiescence through an explicit barrier — settle and a microtask drain — instead of polling a wall clock, so they can assert exact run counts soundly. zig's analogue already reads before it counts. dart's core-effect tests are synchronous, so no window exists. Nothing to port.

### What I got wrong, and why it matters for the next one

The failing assertion was a conjunction: the run count reached the expected number AND the observed value matched. I checked that it was unsatisfied and concluded the wakeup was lost, without asking which half was false. The observed value was already correct. My first probe also ran while eight agents were saturating the machine, so its 8-second bound was not the evidence I treated it as — the same probe on an idle machine, without `-race`, showed zero failures in 400 iterations.

I filed `#lzrstornfanout` for the lazily-rs partial-fan-out failure carried over from last cycle, written to demand a counting probe *first* — it has the same shape as this one, and the honest answer may again be that the test asserts more than the contract promises.
### Re: #lzgoasynclostwake — I was wrong: this is run coalescing, not a lost update — opus-5

**Correcting my own report from last cycle.** I told you lazily-go had a lost wakeup where the effect "stays subscribed holding the pre-write value forever." That was wrong. I measured it this time instead of inferring it from a failed assertion.

A 400-iteration probe under `-race` that records what the effect *actually holds* when the rerun goes missing: **coalesced-but-correct = 4, truly-stale = 0.** Every single case had the effect already holding the written value. Not one instance of a stale value.

**Why it cannot be stale.** `trackDep` and the value read happen inside one `c.do()` closure on the owner goroutine, so registration and read are atomic. Either the write lands first and the body reads the *new* value with the edge registered in the same turn, or it lands after and the edge is already there to schedule a rerun. There is no interleaving that yields a registered edge next to a stale value. What I mistook for a lost wakeup was two logical runs collapsing into one that observes the final value — the same thing `Batch` does on purpose.

**What was actually broken was the test.** It bumped its run counter at the *top* of the body, so the wait for the first run returned as soon as the body *started*, before `TrackSource` had subscribed. A write landing in that gap gets absorbed into the run already in flight, and the count never reaches the next number. The test asserted an exact run count across a boundary where coalescing is legal.

Fixed by bumping the counter **after** the tracking read, so a run count of N is evidence the edge exists and every later write must schedule a real rerun. Same reorder in `TestAsyncEffectDownstreamOfSlotCascade`.

**Verified and mutation-checked**, because a reorder that makes a flaky test pass is exactly the change that could have defanged it:

- 400 iterations of both tests under `-race`: clean. Previously 1-2% red.
- Suppress the effect's edge registration entirely, and it reddens at the first rerun.
- Stop re-registering *after* the first rerun — the precise "deaf after one rerun" defect these tests are named for — and it reddens at the third wait.

`make check` exit 0, CI green.

**Cross-binding check: lazily-go was the only one.** zig and dart both reach quiescence through an explicit barrier — settle and a microtask drain — instead of polling a wall clock, so they can assert exact run counts soundly. zig's analogue already reads before it counts. dart's core-effect tests are synchronous, so no window exists. Nothing to port.

### What I got wrong, and why it matters for the next one

The failing assertion was a conjunction: the run count reached the expected number AND the observed value matched. I checked that it was unsatisfied and concluded the wakeup was lost, without asking which half was false. The observed value was already correct. My first probe also ran while eight agents were saturating the machine, so its 8-second bound was not the evidence I treated it as — the same probe on an idle machine, without `-race`, showed zero failures in 400 iterations.

I filed `#lzrstornfanout` for the lazily-rs partial-fan-out failure carried over from last cycle, written to demand a counting probe *first* — it has the same shape as this one, and the honest answer may again be that the test asserts more than the contract promises.
### Re: #lzgoasynclostwake — I was wrong: this is run coalescing, not a lost update — opus-5
<!-- agent:boundary:fc83b059 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-commit-push use subagents for fan-out tasks" priority go -->
- ⏭️ do [#lzrstornfanout]
- 🚧 do [#lzruffautofixvacuity]
- do [#lzcireachvaranchor]
- do [#lzmsgpackparity]
- do [#lzreadmelinkrot]
- do [#lzvacuousrun]
- do [#lzscenariobodyskip]
- do [#lzspecscenarioids]
- do [#lzassertionblockschema]
- do [#lzdependencyavailability]
- 📌 do [#lzcpptranscribedrunners]
- 📌 do [#lzreactdtsgate]
- 📌 do [#lzegress]
- 📌 do [#lzqueuefamilyflavors]
- 📌 do [#lzwqfixtureconfig]
- 📌 do [#lzingressadapters]
- 📌 do [#kny1]
- 📌 Research https://github.com/Cysharp/R3 and if lazily can have an adapter. Benchmark lazily-cs vs R3.
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#lzrstornfanout] lazily-rs thread_safe_ingress::tests::concurrent_readers_never_observe_a_partial_fan_out failed once in CI with partial fan-out observed: (Some(5), Some(2)) - a reader saw a torn intermediate state where the test asserts it cannot. Surfaced while landing #lzcheckcireachguard; the agent reran the job to green, which does not make it gone, and it has not been reproduced since. Do NOT open by assuming it is a torn read. The sibling finding #lzgoasynclostwake looked exactly like a lost update and turned out to be legitimate run coalescing plus a test asserting more than the contract guarantees, so the first step is a counting probe that records what the reader ACTUALLY observed on failure rather than only that the assertion failed. Run it under -race and under load, since that one only reproduced under -race. Then decide between three outcomes and say which: a real torn read in the thread-safe ingress fan-out, a test asserting atomicity across a boundary the contract does not promise, or a wall-clock deadline masquerading as a correctness failure. Whichever it is, check whether the other bindings' thread-safe shells make the same assertion, and mutation-check the fix by forcing a reader into the window rather than waiting for the roughly one-in-many failure to reappear.
- [ ] 🚧 [#lzruffautofixvacuity] lazily-py pyproject.toml sets fix = true under [tool.ruff], so a bare ruff check REPAIRS the tree it is judging and then exits 0. CI ran exactly that inside poe precommit, which means lint was self-healing to green rather than gating, and formatting was not checked at all because the aggregate contains ruff_fix and no ruff format. CI is fixed as of this cycle by spelling the steps as ruff format --check and ruff check --no-fix, but make lint locally still auto-repairs, so a developer cannot reproduce what CI now enforces. Make the local target non-repairing too, keeping a separate explicitly-named target for the repairing form, and audit the other bindings for the same shape: any linter or formatter whose default is to rewrite is a gate that cannot fail.
- [ ] [#lzcireachvaranchor] [recommended] The CI-reachability guard cannot match a Makefile argument that CI supplies through a shell variable. Recipe arguments are expanded by make -n and land as literals, while a CI-side token starting with a dollar sign has no slash and is dropped by the anchor normalizer, so the two never line up. lazily-cpp hit this: CI genuinely ran the coverage script but passed the manifest path as an environment variable, and the guard reported the target unreachable. It is fail-closed, so it costs a false red and never a false green, and cpp worked around it by naming the same path in the CI step plus an equality assertion so the spellings cannot drift. Decide at the family level whether to keep the strictness, which forces CI to spell what make spells, or teach the normalizer to keep variable NAMES as wildcards. Whichever way it goes, apply it to all nine copies at once, since the script is byte-identical across the family by design.
- [ ] [#lzmsgpackparity] msgpack is a spec MUST for frame codecs and is implemented in only two of nine bindings. protocol.md marks it MUST as the negotiated cross-language binary default on any binary boundary between differing languages, and further states that every fixture's frames MUST round-trip through both json and msgpack for all three IpcMessage variants. Actual state, verified by reading every binding: lazily-rs and lazily-cpp implement it (rs across ipc.rs, ffi.rs, reliable_sync.rs; cpp has include/lazily/codec.hpp and msgpack.hpp). The other seven - py, go, kt, js, cs, zig, dart - mention msgpack in exactly one place each, their interop-peer handshake, where they declare it as an explicit carve_out. That is honest and fails closed, and is the codec-level equivalent of KNOWN_UNCOVERED, so this is a declared gap rather than a hidden one. The problem is that it is invisible where every other parity fact lives: coverage.json has no codec row at all, so the feature matrix cannot show that a MUST-level codec is carved out by seven bindings, and the conformance ladder cannot see it either because all four rungs verify fixture CONTENT replay, not codec round-tripping. Add a codec row per flavor to coverage.json, then add a round-trip conformance obligation so the json/msgpack requirement is actually tested rather than asserted in prose.
- [ ] [#lzvacuousrun] Rungs 2, 3 and 4 all reason about fixtures the run OPENED, so a run that opens ZERO fixtures reports three greens over nothing. Found in lazily-py while closing #lzguardsnotinci and verified concretely there: CI=true pytest tests/test_star_import.py gave 3 passed with every conformance guard satisfied, because an empty opened-set has nothing to complain about. py closed it with a vacuity check in pytest_sessionfinish that fails when the opened set is empty under CI. The same shape appeared independently in lazily-cs as OK-over-nothing output - rung 1 prints coverage OK 0/0 for an empty corpus listing and rung 4 prints 0/0 scenarios across 0 opened fixtures, both exiting 0 - and in lazily-js, lazily-go, lazily-dart and lazily-rs as the coverage script exiting 0 with a SKIP line when the sibling corpus is absent. lazily-cpp is the one binding already immune, because the MIN_FIXTURES and MIN_SCENARIOS floors it added under #lzscenariocoverage fire on an empty manifest. Port a positive-evidence floor to the other eight: every guard must assert it examined a non-zero, expected-magnitude population before it is allowed to report OK. Mutation-check by pointing each suite at an empty corpus and confirming red.
- [ ] [#lzreadmelinkrot] [recommended] Two small README link defects survived the family cross-linking pass, plus one log-noise issue, all found while doing it. First, lazily-zig README line 7 links lazily-py as github.com/btakita/lazily-py, the pre-lazily-hub namespace; GitHub redirects it so it is not dead, but it is the last non-canonical family link in the tree and it will break if that redirect is ever reclaimed. Second, lazily-react's opening paragraph linked @lazily-hub/lazily-js as the relative path ../lazily-js, which only resolves inside a workspace checkout and was dead on GitHub - already fixed in that pass, but it is worth a repo-wide sweep for other relative cross-repo links, since relative links never resolve across repositories. Third, lazily-zig's make check prints a bare failed command line mid-run while the target still exits 0 and every subsequent step passes, which makes a green gate read as red and is exactly the kind of noise that trains people to skim past a real failure. None of the three is urgent; all three are cheap.
- [ ] [#lzscenariobodyskip] The scenario ledger books a scenario at the point the iteration helper YIELDS it, which cannot distinguish a body that ran from one that skipped past. lazily-py proved this empirically against the contract's own probe: with yield-time booking, a continue at the top of the loop body still booked the scenario and rung 4 stayed silent. lazily-kt independently flagged the same thing, and lazily-js hit a live instance - the anchored scenario of collections stableid_alignment.json matched neither arm of an if/else dispatch chain and replayed nothing, invisible to rungs 2 and 3 because key records are keyed by fixture plus block plus key, so a sibling scenario had already marked expect key_equal asserted for the whole file. lazily-py has the reference fix: yield a view that books on first read of the scenario PAYLOAD (steps, ops, frames, expect) and stays silent for label keys (id, name, description), which also stops a by-name lookup from booking every scenario it scans past. Port that booking rule to the other eight bindings, and give every scenario dispatch chain a fail-closed else so an unmatched shape errors instead of passing. Mutation-check with a no-op body, not just a skipped iteration.
- [ ] [#lzspecscenarioids] The shared corpus does not identify scenarios uniformly, which forces every binding's scenario ledger to carry a positional fallback. collections mergecell_algebra.json carries three scenarios with neither an id nor a name - they are distinguishable only by their policy field - so all nine bindings now book them as position #0, #1 and #2 and print a NOTE every run. Separately, 28 fixtures identify a scenario by name while the three stdlib fixtures use id. Positional ids are fragile in exactly the way this ladder cares about: reordering the array silently rebinds every excuse and every ledger entry to a different scenario. Add a stable identifier to the mergecell scenarios and settle on one spelling across the corpus, then drop the positional fallback from the nine ledgers so the fallback cannot quietly become load-bearing. This is a shared-corpus change - a mutation probe against lazily-spec reddens every binding at once, so use a scratch copy and the fixture-dir env var rather than editing in place, and land the corpus change before the nine ledger changes.
- [ ] [#lzassertionblockschema] [recommended] Extend lazily-spec's existing JSON Schema coverage from the wire field to the ASSERTION blocks. Raised by the operator as a question about whether Protobufs would help the key-validation work; the schema instinct is right but protobuf is the wrong instrument here, because codegen replaces the observable dynamic key access every tracker depends on with compiled struct fields, which is exactly the blind spot lazily-go found in #lzconsumednotasserted, and proto3 tolerates unknown fields by design so it would not even give rung 2. The instrument already exists: 14 schemas under lazily-spec schemas, using additionalProperties and required, with tests test_schema_conformance.py carrying both-directions stale-form rejection tests after a real slot_id and base64 drift incident. Two gaps. First, those schemas validate the wire field, while every defect found in #lzassertunknownkeys and #lzconsumednotasserted lived in expect, expected and assertions, which are entirely unschema'd. Second, roughly 13 of 31 fixture areas have a schema at all - collections, ingress, materialization, reactive-graph and temporal have none. Schema the assertion blocks and #lzwqfixtureconfig closes structurally, since visibility_timeout and max_deliveries stop being hardcoded out of band in nine runners.
- [ ] [#lzfixturediscrim] A fixture can be opened, have every key consumed, have every key asserted - and still be unfalsifiable, because its data cannot distinguish the claim it makes. Found in lazily-cs while closing #lzconsumednotasserted: reversing SignalingRoom's roster sort still PASSED, because the canonical anti_spoof_session.json transcript never produces a roster with more than one entry, so no ordering is observable and every emitted frame stays byte-identical. This is the fourth rung - fixture-opened, key-consumed, key-asserted, claim-discriminating - and no runner-side guard can reach it, because it is a property of the corpus data rather than of runner code. Only a mutation against the LIBRARY exposes it. Audit lazily-spec claim by claim: for each assertion key, mutate the behaviour it names and confirm at least one canonical fixture reddens; where none does, either strengthen the fixture data or record the claim as untested with a reason. lazily-cs worked its instance around locally by driving a three-peer room plus a vacuity guard that fails if the roster drops below two, but the corpus fix belongs upstream in lazily-spec so all nine bindings inherit it.
- [ ] [#lzrung2gaps] Conformance runners still outside the unconsumed-key guard entirely, all surfaced while closing #lzconsumednotasserted. lazily-cpp: nine fixture-replaying runners (command, distributed, lossless_tree, receipts, seqcrdt, statechart, stdlib, topic, reactive_graph) carry hand-rolled unknown-key guards instead of the shared AssertionKeys tracker, so they get neither rung 2 nor rung 3, and test_command_conformance.cpp has no unknown-key guard over its expect block at all. lazily-cs: StateChartConformanceTests has no FixtureAssertions tracker because its steps mix op inputs with assertions. Same class in lazily-kt, where bringing MaterializationConformanceTest under the tracker exposed two keys no runner had ever read - default_mode, and observe in deferral_not_deallocation.json. Convert each to the shared tracker so both rungs apply, and mutation-check every conversion rather than trusting the green that follows it.
- [ ] [#lzstructfieldreads] Typed bindings that decode a fixture block into a struct have a blind spot no map-based key tracker can see: an unused local will not compile, but an unused FIELD is silent, so the key counts as consumed at decode time and is then discarded. Found in lazily-go while closing #lzconsumednotasserted, where open_handles and schema_version were both decoded and never read - one of them stayed hidden precisely because its field name collided with a real method. The fix there is TestConformanceStructFieldsAreRead, a go/ast scan of the package's own test sources with a both-directions excuse allowlist. lazily-go is covered. Check whether lazily-rs (serde structs), lazily-cs, lazily-cpp and lazily-zig decode any fixture block into a typed struct rather than reading it through their key tracker, and port the equivalent scan wherever they do.
- [ ] [#lzassertwithseam] The non-equality escape hatch added under #lzconsumednotasserted marks a key asserted BEFORE it runs the caller's check, so a closure that ignores its argument marks the key without proving anything. Flagged by lazily-rs about assert_key_with, and the same seam exists in every binding's equivalent - it is the one place in the new guard held by discipline rather than by the type system, which is exactly the shape of defect this whole ladder exists to remove. Tighten it so the fixture value must actually be consumed: hand the closure a wrapper that records its own read and mark the key asserted only if that read happened, or narrow the API to a fixed set of comparison combinators. Mutation-check by writing a closure that ignores its argument and confirming the guard reddens.
- [ ] [#lzcpptranscribedrunners] lazily-cpp has fully-decoupled runners that hand-transcribe expectations and never open a fixture at all: test_reliable_sync.cpp, test_family_sync.cpp and test_outbox_store.cpp. The #lzassertunknownkeys guard cannot touch them because there is no assertion block to guard — a transcription has no unconsumed key. The cpp coverage script already reports the corresponding fixtures as uncovered rather than claiming them, so the ledger is honest, but the transcribed values can silently diverge from the corpus forever and the tests keep passing against the old numbers. This is the same defect the zig queue-family runner had before c103070 replaced its transcriptions with a real replay. Convert each to replay the canonical fixtures, then confirm the runtime manifest count rises and remove the matching KNOWN_UNCOVERED entries. Also note the root-level IPC corpus in cpp has no runner at all and is excused the same way.
- [ ] [#lzreactdtsgate] lazily-react ships src/*.d.ts and a `types` export but its `make check` is only `node --check` syntax validation plus `node --test` (18 tests), so nothing typechecks the declarations it publishes. Same hole that let lazily-js ship four TS2749 errors in transport.d.ts undetected until this cycle. Port the gate landed in lazily-js 553d840: a declaration-only tsconfig (include src/**/*.d.ts, noEmit, strict, skipLibCheck:false, allowJs:false), an npm typecheck script wired into both `make check` and CI, plus CI's non-empty-program assertion, since a gate whose include matches nothing also exits 0. Mutation-check by breaking one declaration and confirming a non-zero exit before the tests run.
- [ ] [#lzqueuefamilyflavors] Ship the thread-safe and async queue-family flavors in lazily-js, lazily-zig and lazily-cs - the 18 cells that coverage.json rows 17/18/20/21/23/24 show absent. IMPLEMENTATION HAS LANDED IN ALL THREE BINDINGS: lazily-js 67a0013, lazily-cs f056dd8, lazily-zig c103070 (the last one mutation-checked with 15 defects across the shared core and all six new shells, green on 0.15.2 787/791, 0.16.0 849/849 and master 849/849, CI 30523011815 green). Do NOT re-implement. What REMAINS is the ledger and the plumbing: flip rows 17/18/20/21/23/24 for the JS, Zig and C# columns in lazily-spec/coverage.json, regenerate the twelve README/doc coverage tables via sync-coverage.mjs (--check is the gate), fix lazily-zig README line ~92 which claims every spec row is shipped for the Zig column while the generated table still shows those six as absent, and bump the superproject gitlinks for lazily-js, lazily-cs and lazily-zig. Verify each mark against that binding's own 3x3 ledger and replayed corpus before flipping it, not against the commit message.
- [ ] [#lzwqfixtureconfig] Put the WorkQueueCell lease configuration into the canonical workqueue_*.json fixtures' initial block: visibility_timeout and max_deliveries are absent, so every binding hardcodes them out of band (10, and max_deliveries 3 for competing_delivery vs 2 for lease_deadletter) and a new binding has to read lazily-go's runner to discover them. Add the fields to the fixture and the schema, then update the nine bindings' runners to read them, so a fixture change cannot silently mean something different in each binding.
- [ ] [#lzegress] Design and implement the egress family as the mirror of ingress (#designimplementtransport): an EgressCore owning sequence assignment, the unacked window, the ack watermark, and retry budget, with sync + thread-safe + async shells exposing pending/inflight/acked_through/retry as Computeds and ONE Effect per attached transport as the sink attachment. An Effect alone cannot be the family — it has no delivery state — so the Effect attaches, the core decides. Reuse Outbox/RelayCell for conflation+backpressure and SpillStore/DurableOutbox for durability rather than reimplementing them; add a generation fence on the SEND side so a reconnected producer cannot replay under a stale incarnation. Spec + formal + conformance corpus alongside, as ingress did.
- [ ] [#lzingressadapters] Extend the shipped IngressCell family with boundary-ingress adapters learned from the agent-doc 0.35.100 build; do not create a second ingress primitive. Phase 1: specify an event-channel-first adapter contract for sync, thread-safe, and async sources, with RPC/poll adapters allowed only at external non-reactive boundaries and all ingress updating Sources. Phase 2: provide atomic snapshot-plus-event bootstrap with monotonic cursors, gap detection/cold replay, generation fencing and hot resubscription so controller or producer replacement cannot strand a graph on an old stream. Phase 3: derive readiness, authority, freshness, membership, delivery convergence and template-style validation in Computeds (extract every reactive read into the computed); keep subscription lifecycle, I/O, acknowledgements, retry and commits in Effects. Include keyed SourceMap membership and one-shot durable delivery receipts so temporarily reaching zero subscribers is not mistaken for convergence. Phase 4: map agent-doc onto the adapters as the reference integration, then add spec, formal model, cross-binding conformance fixtures and mutation probes for restart during bootstrap, stale-generation events, cursor gaps, zero-member delivery, duplicate acknowledgements, backpressure and no-torn-observation. Reuse #designimplementtransport semantics and coordinate with #lzegress rather than duplicating either core.
- [ ] [#kny1] Please help me setup trusted publishing with npm.
- [/] [#lzjspublish] After the operator completes npm security-key reauthentication in the Brave package-access tab, configure the GitHub Actions trusted publisher for lazily-hub/lazily-js using workflow release.yml with npm publish permission; rerun release job 30309980372 for v0.30.1 and verify the registry resolves 0.30.1.
- [ ] [#lzgdbinding] Implement lazily-gd, a pure-GDScript binding, as the 10th coverage.json column per tasks/software/plan-lazily-gd.md. Staged entry copying lazily-cs: rows 0 (reactive graph) + 15 (stable-id) first, KNOWN_UNCOVERED populated and shrinking per phase. The one novel problem is that RefCounted has no cycle collector — no other binding faces this — so back-edges are WeakRef, which makes ctx.scope() the SOLE strong owner of effects and requires prune-on-propagate before any edge-count assertion and an edge index keyed on get_instance_id(). Port the runtime-manifest guard (build/conformance-fixtures-loaded.txt + check-conformance-coverage.sh with EXTS=.gd) in the SAME phase as the runner, and mutation-check it before trusting green. Efficient loading is a Phase-1 exit criterion, not a late phase: a load-graph test asserting the kernel loads no other family, since preload() is eager+transitive and retrofitting layering after six families exist is the expensive order. Ship as addons/lazily/ with the runner and 134 fixtures export-excluded. BLOCKED on the operator choosing a minimum Godot 4.x version.
- [ ] [#lzcppwasm] Give lazily-cpp an Emscripten/wasm32 target with a replayed feature-coverage matrix and benchmarks per tasks/software/plan-lazily-cpp-wasm.md. Measured: the reactive core is already wasm-clean (cell.hpp closure = 8 headers, zero POSIX and zero threads; same for collections/keyed_order/statechart), work_queue.hpp needs -pthread, and the lazily.hpp umbrella (46 headers) drags in shm_open/mmap via transport.hpp — so three explicitly-labeled build tiers (core / threaded / native-only-excluded), not one build that silently drops features. Add lazily/core.hpp first to name the narrow-include contract. The coverage matrix must be generated from the runtime fixture manifest, never hand-authored: lazily-cpp's own queue tests were previously hand-transcribed rather than replayed, which is exactly the drift to avoid repeating on a new target. Wasm benchmarks go in a SEPARATE BENCHMARKS.md section with their own environment line — native -O3 numbers are not comparable. Do NOT split the large headers (context.hpp 2117 lines etc.): header size costs compile time not runtime loading, the dependency hazard is already solved at file granularity, and it would churn a released library for no behavioral gain. Do NOT flip coverage.json — it tracks language bindings, not build targets of one binding.
- [ ] [#lzcoveragelayout] Restructure the coverage.json feature-table layout for a growing language axis per tasks/software/plan-lazily-coverage-table-layout.md. Measured: the feature column is 68% of row width at mean and 84% at max (feature text mean 86 / max 203 chars vs ~40 for all nine marks combined), so a 10th language adds ~4 chars to a row already 130-250 wide — the language axis is NOT the problem. Reject transposing (64 columns) and reject splitting by language: sync-coverage.mjs injects the same full matrix into all 12 targets, and per-language tables would destroy the cross-binding parity view that check-conformance-coverage.sh exists to protect. Phase 1 (land BEFORE the GDScript column, independent of the rest): add id + short label per row and move the 86-203 char normative prose to footnotes or into cell-model.md/protocol.md, taking the table from ~250 to ~60 chars wide so the layout scales past 15 languages. Phase 2 with it: split the '— absent OR not applicable' mark, since GDScript's parked thread-safe family is a different fact from unwritten work. Phases 3-4 (follow-up): author a family field — 54 distinct prefixes across 64 rows means families CANNOT be inferred from the current text — then a family x language roll-up with a stated roll-up rule. Layout and mark-value changes stay separate commits; sync-coverage.mjs --check stays the gate.
- [ ] [#lzdependencyavailability] Implement tasks/software/plan-lazily-dependency-reactive-availability.md: model every late-bound dependency as a stable per-key reactive with explicit unavailable/available state so Computeds subscribe before first publication; cover single-threaded, thread-safe, async, and binding parity, then remove Agent Doc compatibility workarounds.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#lzstdlibmatrixrelease] Implement Phase 5 of tasks/osoftware/plan-lazily-stdlib-conformance-rollout.md: run the capability-driven all-peer/all-channel matrix, enforce CI and mutation gates, publish coverage evidence, and release bindings inside-out only after every claimed cell is green.
- [/] [#lzjsnpmtrustedpub] [operator-action] lazily-js repository workflow/OIDC/provenance is ready, but npm rejects publication until trusted-publisher setup is completed. NEXT: authenticate npm, register `@lazily-hub/lazily-js` for `lazily-hub/lazily-js` workflow `release.yml` with no environment and allowed action `npm publish`, verify with `npm trust list @lazily-hub/lazily-js`, then prove the next version tag.
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
