---
name: reference_lazily_spec_shared_corpus_concurrent_mutation_probes
description: "All nine lazily bindings resolve the conformance corpus from ONE ../lazily-spec sibling, so a mutation probe written there surfaces as unrelated red in every other binding running at the same time."
metadata: 
  node_type: memory
  type: reference
  originSessionId: 2c19ab51-2bf2-4bae-b1f8-bc8cfb5608bb
  modified: 2026-07-30T16:05:32.954Z
---

Observed 2026-07-30 during the `#lzassertunknownkeys` fan-out. A lazily-zig `make check`
failed on `bogus_unimplemented_key` in `presence/presence.json` — a probe key that agent
never wrote, and which had vanished from `lazily-spec` by the time it looked. Another
concurrently-running agent had briefly written a mutation probe into the **canonical**
corpus instead of a scratch copy.

**Why it bites:** every binding resolves fixtures from the single sibling checkout
`../lazily-spec/conformance` (or `LAZILY_SPEC_CONFORMANCE_DIR`). There is no per-binding
copy of the corpus. So a probe key that exists for even a few seconds reddens whichever
other suites happen to be mid-run, with an error naming a fixture and key the reader has
never seen — which reads as a real regression in *their* binding.

**How to apply:** when fanning out mutation checks across bindings, instruct every agent to
copy the corpus to a scratch dir and point the runner at it via
`LAZILY_SPEC_CONFORMANCE_DIR`, never to edit `../lazily-spec/conformance` in place — even
transiently, even with a guaranteed revert. Then verify afterwards with
`git -C src/lazily-spec status --porcelain` plus a grep for the probe key names across
`conformance/`; a clean tree is not proof on its own, because the window is what does the
damage, not the residue.

**When an unfamiliar fixture/key reddens a binding mid-fan-out:** suspect this before
suspecting the binding. Re-run once; if it passes and the key is absent from the corpus, it
was a neighbour's probe. Do not "fix" the binding.

Related: [[feedback_never_dispatch_concurrent_agents_into_one_repo]] — the same class of
hazard one level up, where the shared resource is a repo rather than a corpus.
