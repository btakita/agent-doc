---
name: reference_corpus_perturbation_finds_vacuity_static_passes_cannot
description: "Perturbing each fixture key one at a time in a scratch corpus is the only pass that finds a key which is read, typed, asserted and still switchable off — and the audit detector itself must be validated against pre-fix sources first"
metadata: 
  node_type: memory
  type: reference
  originSessionId: 5b3f7f45-0ae5-40c6-8b7f-79100a00d31b
  modified: 2026-08-03T04:45:12.487Z
---

Found across the lazily nine-binding sweep (2026-08-03, `#lznullformblind`).

**The pass:** change every key of every top-level `assertions` block, ONE AT A TIME,
in a **scratch copy** of the corpus, and check the suite reddens. Never edit
`lazily-spec` in place — that reddens every other binding concurrently
([[reference_lazily_spec_shared_corpus_concurrent_mutation_probes]]).

**Expected result:** everything reddens EXCEPT keys declared `assertions.prose`,
which are green by design (asserting a paragraph pins wording — see
[[reference_lazily_prose_key_convention]]). Anything else staying green is a finding.
cpp: 42/63 red, all 21 green ones declared prose. rs: 86/107 red, same shape.

**What only it finds:** lazily-go's `anti_spoof_session.json` keys
`forwarded_from_is_server_registered` / `roster_sorted_ascending` were gated
`if want != nil && *want { check }`. Read, typed, in a struct, three live grep
references each — consumption, assertion and prose-discharge rungs all green. Setting
either to `false` in the corpus **silently retired the rule**. lazily-rs had the same
three keys *excused* rather than gated, identical effect.

**The detector hunting this has the defect it hunts.** Validate it against
`git show HEAD:` / pre-fix sources and confirm it flags known instances BEFORE
believing any clean result — a detector reporting zero is indistinguishable from a
broken one. Real instances: kt's reported zero (`git archive --strip-components` off
by one, extracted 1 file); cpp's first two reported clean, then five defects surfaced,
including **comment text parsed as identifiers — so the best-commented vacuous
assertions escaped most reliably**; go's untainted the fixture through
`f, ok := load(...)` rebinding.

**Related shapes in the same family:** an `assertions` block no runner *binds* is
invisible to every rung (its keys aren't unread — nothing reads them); a scenario's
own label used as a branch *selector* is not an assertion; an assertion ordered ahead
of the work it describes, or behind an earlier fatal gate, is unreachable for its own
falsifying input. See [[reference_lazily_conformance_delivery_three_failure_modes]].
