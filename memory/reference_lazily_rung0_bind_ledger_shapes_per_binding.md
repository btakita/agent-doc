---
name: reference_lazily_rung0_bind_ledger_shapes_per_binding
description: "How the rung-0 assertion-block bind ledger is wired in each lazily binding, and the two things that make it non-vacuous"
metadata: 
  node_type: memory
  type: reference
  originSessionId: 0a3317a3-3f7b-4666-9d4c-3ec682ae6da6
  modified: 2026-08-03T06:16:53.800Z
---

The rung-0 ledger (`#lznullformblind`) needs a DECLARING side independent of the
binding side, or it is the recorder agreeing with itself.

- **Declare from the bytes** — the fixture loader (`spec_read_to_string` rs,
  `Path.read_text` patch py, `specReadFile` zig, `SpecCorpus.Load` cs, the guard
  script reading the corpus off disk in js). Never from what a runner hands over.
- **Bind at the tracker constructor** — `Expect::new` rs, `TrackedBlock.__init__`
  py, `AssertionKeys.init` zig, `FixtureAssertions` ctor cs, `instrumentBlock` js.
- **Match by CONTENT digest, never by the `where` label.** Runners spell labels
  inconsistently and several book every per-frame block under the bare name
  `assertions`, so a label-keyed ledger collapses them and reports green.
- **Digest by a structural walk, not by raw text or `stringify`.** zig's
  `std.json.stringify` API differs across its three pinned toolchains; cs's
  `GetRawText()` carries fixture whitespace so a rebuilt block reads as unbound.

Transport: rs/js use a separate ledger file + env var; zig and cs ride the
EXISTING manifest under an `@block` / `blocks-` prefix (no second file, no second
env var, no second CI wiring) — but then the scenario parser must skip those
prefixes or it reports them as scenarios of a fixture named `blocks-bound`.

Always validate: stub out the bind recorder and confirm the guard NAMES sites
instead of reporting clean. See [[reference_corpus_perturbation_finds_vacuity_static_passes_cannot]].
