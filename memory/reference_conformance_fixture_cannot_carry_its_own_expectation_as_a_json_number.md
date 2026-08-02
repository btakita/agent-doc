---
name: reference_conformance_fixture_cannot_carry_its_own_expectation_as_a_json_number
description: "A JSON fixture testing >2^53 integer handling must carry wire + expectation as strings/hex — a bare number is rounded by the parser LOADING the fixture, so the test agrees with a rounding decoder."
metadata:
  type: reference
---

Found 2026-08-02 designing `conformance/codec/nodeid_exact_range.json` (`#lzspecdecoderbound`),
the fixture for "a decoder that cannot represent a received `NodeId` exactly MUST reject
the frame rather than round it".

**The trap:** the fixture is itself JSON. Spell the expected id as a bare
`9007199254740993` and a JS/Dart-web runner's `JSON.parse` rounds it to
`9007199254740992` *while loading the file* — before a single assertion runs. The runner
then compares a rounded expectation against a rounded decode and **passes against exactly
the decoder the fixture exists to catch.** Go has the same shape via `map[string]any`
(numbers land as `float64`).

**The shape that works:**
- `wire_json` = the frame as RAW TEXT; `wire_msgpack_hex` = the frame as hex.
- `expect.node_id_decimal` = a decimal STRING; runners compare the decoded id's decimal
  rendering, never a number.
- The runner passes `wire_json` to the codec's own entry point — never a pre-parsed
  object, or the parse that would round moves outside the code under test.
- A spec-side guard walks the parsed fixture and fails on any integer above 2^53−1
  appearing anywhere as a JSON number, so the rule cannot rot.

**Outcome model:** per-binding answers differ (u64 in rs/zig/cs, unbounded in py, i64 in
kt/go/cpp, 2^53 in js, platform-split in dart), so scenarios are `exact` or
`exact_or_reject`, never one boolean. `exact_or_reject` alone is satisfiable by a runner
that decodes nothing and reports "rejected" — so every codec needs at least one `exact`
control scenario at 2^53−1, and runners pin the accept/refuse COUNTS exactly rather than
"at least one". See [[reference_lazily_conformance_delivery_three_failure_modes]] and
[[feedback_conformance_tests_drive_real_behavior_not_runner_bookkeeping]].
