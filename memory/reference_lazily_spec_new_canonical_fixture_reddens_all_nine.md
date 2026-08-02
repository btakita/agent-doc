---
name: reference_lazily_spec_new_canonical_fixture_reddens_all_nine
description: "Adding ONE fixture to lazily-spec/conformance immediately fails every one of the nine bindings' check-conformance-coverage.sh — the change is never spec-only, it is a ten-repo change."
metadata:
  type: reference
---

Observed 2026-08-02 landing `conformance/codec/nodeid_exact_range.json` (`#lzspecdecoderbound`).

Writing the file into `lazily-spec/conformance/` and running each binding's
`scripts/check-conformance-coverage.sh` produced, in all nine:

```
ERROR: canonical fixture 'codec/nodeid_exact_range.json' was NOT opened by the suite.
conformance coverage FAILED: 1 problem(s)
```

**Why:** the coverage guards read the canonical corpus directly at
`../lazily-spec/conformance` (or `LAZILY_SPEC_CONFORMANCE_DIR`) and fail on any fixture
in a required area that is neither replayed nor listed in `KNOWN_UNCOVERED`. The
vendored mirrors (`sync-conformance-fixtures.mjs`, only py/dart/rs/go) do NOT gate this —
that script only checks mirror⇄canonical byte equality in the mirror→canonical direction,
so a new canonical file is invisible to it. Mirroring is therefore not the blast radius;
the guards are.

**How to apply:** budget a new canonical fixture as ~10 repos, not 1. Per binding you
need: a runner, whatever build/test registration that binding uses (`Makefile` target,
`tests/CMakeLists.txt`, `src/root.zig` re-export, …), a `MIN_FIXTURES` floor bump where
the script keeps one, and a `coverage.json` row + `make coverage-sync` (which rewrites a
README in every sibling checkout, including [[reference_lazily_release_targets]] repos
that carry no code change). `check-coverage-claims.mjs` verifies a ✅ against each
binding's own ledger, so the row cannot be filled in ahead of the runners. Deciding to
"land the spec now and adopt per binding later" is not available — it leaves nine red
gates. See [[reference_lazily_spec_shared_corpus_concurrent_mutation_probes]] for the
concurrency hazard on the same corpus.
