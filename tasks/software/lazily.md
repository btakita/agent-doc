---
agent_doc_session: d818612d-3614-4c43-b5ec-83923406aa46
resume: 04411a16-b35d-4e62-b38b-a4e650f0f778
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
<!-- agent:boundary:c6ef3fb1 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-commit-push use subagents for fan-out tasks" priority go -->
- ~~do [#lzzigcanonrunner]~~
- do [#lzpartialmarkadjudicate]
- 📌 do [#lzzigspecdiroption]
- 📌 do [#lzktcorpusnotgradleinput]
- 📌 do [#lzledgeragreementaudit]
- 📌 do [#lzzigdiffmutant]
- do [#lzperturbaudit]
- do [#lzspecschemasoverride]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#lzzigforkhlcpeer] lazily-zig's SeqCrdt.fork copies the SOURCE's whole Hlc (, seq_crdt.zig ~line 130), so a forked replica keeps the source's  and mints stamps attributed to the wrong peer; lazily-rs forks with  (seq_crdt.rs:97-103), a fresh clock for the new peer. Two independent differences ride on that one line: (a) the stamp's peer field, where rs is plainly right, and (b) whether a fork carries the source's causal position forward at all, where zig's carry-forward looks MORE correct than rs's reset to zero. seqcrdt_convergence.json pins neither — every fork in it is followed by an op whose  exceeds the source's last_wall, so  takes the  branch and the logical counter resets regardless, and no scenario produces an equal-(wall, logical) tie whose peer tiebreak would separate the two. Decide (a) and (b) in lazily-spec first, then add a discriminating scenario (concurrent fork edits at the SAME  on both replicas, converging through a peer tiebreak) and fix whichever bindings disagree. Adding that scenario reddens all nine bindings at once, so budget it as a family-wide change, not a zig-local fix.
- [ ] [#lzspecgcreferencedtombstone] collections/textcrdt_convergence.json's  does not pin the half its name claims. It deletes index 2 of "abc" — the LAST character — so nothing references the tombstone as a left origin, and a  mutant that drops the  conjunct entirely collects the same 1 element and stays GREEN in lazily-zig (mutant M6 of the #lzzigcanonrunner sweep; the other five mutants all redden). The conservative-GC rule is real and every binding implements it; no fixture in the corpus falsifies its absence. Add a scenario that deletes an INTERIOR character and asserts gc(stable=true) collects 0 while the tombstone count stays 1, then re-run the mutant against every binding — expect finds, since a binding could have shipped the same unreferenced-only reading. This is a lazily-spec fixture change, so it reddens whichever of the nine got it wrong; that is the point.
- [ ] [#lzpartialmarkadjudicate] check-coverage-claims.mjs (as of lazily-spec d1c1a82) reports three partial marks that are INDISTINGUISHABLE FROM SHIPPED on ledger evidence: GDScript on row reactive-graph (reactive-graph/{transitive_invalidation_reaches_depth,signal_materializes_without_a_read,signal_materializes_once_per_batch,dispose_signal_reverts_to_lazy}.json), Zig on reactivemap-core-single-threaded (collections/{cellmap_independence,cellmap_atomic_move}.json), and C# on reliable-sync (reliable-sync/{coalesce_bounds_outbox,liveness_lease_eviction}.json). None of the eight fixtures is in any KNOWN_UNCOVERED and no binding has a scenario- or block-level excuse. Adjudicate each ONE AT A TIME with binding-specific evidence the spec repo does not have: confirm from the binding's runtime manifest that it really replays each fixture, then decide whether the feature is complete or the shortfall is real but unpinned. Outcome per row is either (a) promote the mark and re-run sync-coverage.mjs — remember a row flip can carry its FAMILY roll-up cell with it and rewrites README.md in 11 sibling repos, so it is a 12-repo commit — or (b) cite a fixture that actually pins the gap, or add a scenario-level excuse, so the tilde becomes backed instead of merely unverified. Do NOT bulk-promote: tilde legitimately means 'implemented, but not this wire' and the guard deliberately cannot tell the two apart.
- [ ] [#lzzigspecdiroption] lazily-zig has NO corpus-dir override: SPEC_DIR in src/lazily/reactive_map.zig is a comptime const used as SPEC_DIR ++ '/x.json', so the #lzdefaultmodeuniform probe had to temporarily edit the constant and restore it. Every future corpus probe in this binding pays that cost, and a probe that edits the shared ../lazily-spec checkout instead reddens all nine bindings concurrently. Add a '-Dspec-dir' build option (or a runtime env read) threaded to every fixture path, and keep the default unchanged. Compounding trap to preserve: specFixturesPresent() makes the tests 'return error.SkipZigTest' when the path is unreadable, so a probe that changes the PATH silently converts red into skip — the option must change which bytes are read, never whether they exist.
- [ ] [#lzktcorpusnotgradleinput] lazily-kt's conformance corpus is NOT a declared input of the gradle 'test' task, so editing a fixture leaves 'test' UP-TO-DATE and the run reports green having replayed nothing — a false green that defeats any corpus-perturbation probe. Compounding it, tasks.test wires no environment(...), so the test JVM inherits the DAEMON's environment and a daemon started before LAZILY_SPEC_DIR was set serves the stale path. Today the only defence is remembering '--no-daemon --rerun-tasks'. Declare the corpus directory as a task input so a fixture change invalidates the task, and pass the corpus-dir variable through environment(...) explicitly. Definition of done: flip one assertion key in a scratch corpus and prove a plain 'make check' (no flags, warm daemon) reddens.
- [ ] [#lzledgeragreementaudit] A KNOWN_UNCOVERED entry in a binding's shell coverage guard does NOT stop a test host that DISCOVERS fixtures by enumerating the corpus directory from trying to replay them — the two ledgers describing the same absence can disagree silently. lazily-cs was red on HEAD for exactly this (CI 31298919123): collections/registers_convergence.json was declared uncovered in the script while CollectionsConformanceTests crashed on it with a bare KeyNotFoundException naming neither the fixture nor the reason. Fixed there in b8cdb7e by adding the Unsupported entry and making the shape dispatch fail closed. Audit the other eight bindings for the same split: for each, list the fixtures in KNOWN_UNCOVERED, check whether the runner enumerates or names fixtures, and confirm an enumerating runner has a matching in-host ledger entry plus a fail-closed default on an unrecognised fixture shape. All eight are currently green, so this is about the NEXT fixture added to the shared corpus.
- [ ] [#lzzigdiffmutant] lazily-zig's lossless-tree replay does not pin the anti-entropy FILTER, only convergence: mutating LosslessTreeCrdt.diff to ignore the partner frontier (ship every op in the log) reddens NONE of the nine fixtures, including non_contiguous_anti_entropy.json whose subject is a replica with a gap. deliver.only indexes into the diff, so a diff whose contents change should select different ops and change what the gapped replica renders (hole_before_repair expects b='013') — it does not, and the reason is not yet established. Either the runner's deliver path is not exercising the frontier as intended, or applyUpdate's idempotence absorbs the difference; find out which, then make the fixture discriminating (or record honestly that convergence is all it can pin). Contrast the at_byte char-offset mutant, which reddens six of the nine.
- [ ] [#lzperturbaudit] The corpus-perturbation audit that #lzoverrideallrunners existed to unblock has NOT been run. All five bindings (cs/go/dart/js/py) can now be pointed at a scratch corpus, but each has been probed with exactly ONE fixture, the one proving its seam. lazily-rs's AGENTS.md documents this audit as the pass a static detector cannot replace. For each of the five, walk the conformance families and perturb at least one fixture per family, recording which runners redden and which are convergence-only (a fixture that cannot be made to fail by flipping a declared expectation is pinning nothing). Expect finds of the #lzzigdiffmutant shape: an assertion that survives a mutation because an earlier assertion fires first, or because the runner never reaches it. Definition of done: a per-binding table of family -> perturbed key -> reddened runner, with every non-reddening cell explained rather than dropped.
- [ ] [#lzspecschemasoverride] LAZILY_SPEC_CONFORMANCE_DIR redirects the conformance CORPUS only; lazily-spec/schemas always resolves to the canonical sibling in go (specSchemasDir, 2 sites in ipc_conformance_test.go), js (schemasRoot/schemaPath, used by schema-conformance + lossless-tree-crdt + ipc) and cs (5 schema roots derived from SpecCorpus.Root via '..'). This is deliberate today because a corpus scratch copy carries no schemas/ sibling, but it means a probe that needs to perturb a SCHEMA has nowhere to point except the shared ../lazily-spec checkout, which reddens all nine bindings at once. That is the exact defect #lzoverrideallrunners just fixed, one level up. Add a spec-ROOT override (or a second LAZILY_SPEC_SCHEMAS_DIR) threaded through the same seams, default unchanged, fail-closed on an unreadable explicit value. Audit dart/py/kt/rs/cpp/zig/gd for the same split while there. Definition of done: flip one field in a scratch schemas copy and prove a schema-validating runner reddens without touching the canonical checkout.
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
