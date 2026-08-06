---
name: feedback_per_call_site_count_is_not_conformance
description: A guard written at the call site is a stopgap; the obligation belongs in the tracker where the next author cannot forget it
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 84b8a6c1-d889-42eb-9ef5-9773cf74dcd8
  modified: 2026-08-03T07:33:34.013Z
---

When a conformance check needs a new invariant, putting it at the CALL SITE — a
field count, an explicit key list, a comment saying "remember to also check X" —
is the wrong shape even when it is correct today. It relies on the next author
remembering, which is exactly the property these rungs exist to remove.

`#lzsubblockkeyset` is the worked example. lazily-zig and lazily-cs first "fixed"
the object-valued-key hole with `expectEqual(5, fields.count())` beside the
descriptor check. Moving the same obligation into the tracker (a finish-time guard
that fails any object-valued key consumed without a key-set check) reddened 31 cs
tests — the call-site count had covered 1 of 18 shapes.

**Why:** a call-site guard is invisible to every other call site, and a check that
cannot fail reads identically to one that passes. A tracker-level guard converts
"the author remembered" into "the run reports it".

**How to apply:** when writing a conformance guard, ask where a NEW instance of the
defect would be caught. If the answer is "wherever someone writes the check again",
push it down into the tracker/harness and let it name the existing violations. Then
validate the guard itself by neutering its bookkeeping and confirming it names sites
rather than reporting clean. See
[[reference_lazily_object_valued_assertion_keys_live_in_expect_blocks]] and
[[reference_corpus_perturbation_finds_vacuity_static_passes_cannot]].
