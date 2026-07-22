---
name: reference-lazily-conformance-delivery-three-failure-modes
description: "A lazily binding can report green while executing zero fixtures in three distinct ways; only one is catchable by an absence guard, so always add a positive assertion that fixtures actually ran"
metadata: 
  node_type: memory
  type: reference
  originSessionId: 9d435e15-6b59-41e8-9d3e-16220a325346
---

Repaired family-wide 2026-07-19 (`#lzdartobservercow`). Before that, **every
lazily binding except `lazily-rs` could report a green conformance suite while
executing nothing**, by three mechanisms that need three different fixes:

| Mechanism | What CI sees | Absence guard catches it? | Found in |
|---|---|---|---|
| CI never clones the spec sibling | Every suite skips silently | **Yes** | `lazily-go` (whole suite, since those tests landed), `lazily-kt` (`test` job), `lazily-cpp` (no clone at all), `lazily-js` (no `ci.yml` at all — conformance ran only at release) |
| Cloned, but a bundled copy resolves **first** | Stale duplicate runs and passes | **No** — nothing is ever absent | `lazily-dart` (12 files; 2 never consulted the sibling at all) |
| Fixtures bundled with no sibling path | Drifted duplicate runs and passes | **No** | `lazily-cpp` (33 vendored), `lazily-kt` (87 bundled) |

**Why:** an absence guard (`test -d ../lazily-spec/conformance || exit 1`) only
defends the first row. The other two never trigger it, because the fixtures are
present — just not the canonical ones. A skip-if-absent runner with no guard is
worse than no runner: it names itself after work it is not doing.

**How to apply:**
1. Mirror `lazily-rs` (`#lzspecconf`): one sibling-relative constant
   (`../lazily-spec/conformance/<area>`), skip-with-explicit-message on absence,
   CI clones the sibling, CI asserts the directory exists.
2. **Add a positive assertion** — the part rs does not have. Assert fixtures
   actually ran: `lazily-go` greps replay output for zero-skips, `lazily-cpp`
   uses `REQUIRE_FIXTURES_LOADED(n)` on distinct paths opened, `lazily-kt`
   records every load and asserts >= 96 across 20 areas, `lazily-js` greps the
   summary for `fail 0`, `skipped 0`, `pass > 0`.
3. **Prove the guard bites** before believing it. Every agent that verified by
   running with no sibling found real failures where the old path passed clean
   (js: 17 hard failures; kt: exit 1 with 105 failures; cpp: skip-77 / abort /
   exit-1 each triggered deliberately).
4. Prefer **sibling-first with no fallback**. cpp/js/kt deleted their copies
   outright with nothing lost. dart kept mirrors but inverted precedence and
   added a drift guard comparing *parsed* fixtures (bytes fail on reformatting).

**Corrections to earlier belief, both found by measuring:**
- The kt drift (`crdt_sync_frames.json` 2.6K vs 2.1K) was **already fixed** by a
  prior session. At HEAD all 87 kt fixtures were byte-identical to canonical, and
  cpp's 33 vendored files were too. kt's real defect was **coverage**, not drift:
  9 fixtures (8 `Command`, 1 `Receipt`) sat behind silent `if (!present) return`
  guards in areas kt never bundled, asserting nothing in CI for their whole life.
  A byte-compare script is structurally blind to this — it only checks fixtures
  that *are* bundled.
- Do not trust RTK's `diff` for this work; see
  [[feedback-rtk-masks-make-check-exit]].

Related: [[reference-lazily-release-targets]],
[[feedback-never-dispatch-concurrent-agents-into-one-repo]].
