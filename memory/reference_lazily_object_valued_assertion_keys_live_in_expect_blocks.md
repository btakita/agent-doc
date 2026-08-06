---
name: reference_lazily_object_valued_assertion_keys_live_in_expect_blocks
description: "Auditing object-valued assertion keys by grepping top-level `assertions` blocks undercounts the defect ~20x — the trackers guard expect/expected/per-step blocks too"
metadata: 
  node_type: memory
  type: reference
  originSessionId: 84b8a6c1-d889-42eb-9ef5-9773cf74dcd8
  modified: 2026-08-03T07:33:26.313Z
---

Enumerating the lazily corpus for object-valued assertion keys finds exactly TWO
(`arena_blob.json:descriptor`, `codec/nodeid_exact_range.json:outcomes`) — but only
because that scan looks at top-level `assertions` blocks. Every binding's
assertion-key tracker ALSO guards `expect` / `expected` / per-step / per-scenario /
per-frame blocks, and those carry the overwhelming majority. Measured when the
finish-time guard was landed (`#lzsubblockkeyset`, 2026-08-03):

| binding | what the guard named |
|---|---|
| cs | 31 tests red, 18 distinct key shapes |
| py | 245 report lines / 59 fixtures, 26 keys |
| js | 124 keys across ~30 call sites |
| go | 19 keys / 26 call sites |
| cpp | 14 shapes, 1467 block instances when neutered |
| kt | 31 of 73 planted+opened fixtures were unnoticed pre-fix |

Recurring shapes: `invalidates`, `scopes`, `receipts`, `reads`, `subscriptions`,
`values`, `handle_stable`, `observe`, `states`, `present`, `discovery`,
`projection`, `frame`, `state_after`, `converged_nodes`, `text_on`,
`version_vector_on`, `order_on`, `get`/`get_on`, `final_state`, `after_publish`,
`authority`, `retry`, `dependents_of`, `readable`, `read`. Ingress nests four deep
(`invalidates.scopes.<key>.<reader kind>`).

**Do not scope this class of audit from a corpus grep.** Land the tracker-level
guard first and let it name the sites — the grep answer and the guard answer differ
by more than an order of magnitude. See [[reference_corpus_perturbation_finds_vacuity_static_passes_cannot]].

Related: [[reference_lazily_corpus_perturbation_harness_per_binding]],
[[feedback_conformance_tests_drive_real_behavior_not_runner_bookkeeping]].
