---
agent_doc_session: tsift-v0.1
resume: f81f81a1-f2aa-4dee-910a-835965b1a074
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#advance-review': Go through the review items. Add actionable backlog items to complete or advance the review items.,
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#bugs-observed': Please add any bugs from observed behavior to the backlog.,
  '#code-review': Please review tsift codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. '#follow-up-backlog'
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: go
---

## Status

<!-- agent:status patch=replace -->

complete
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260730-034115.md`*

Compacted content:
- Archived 1 response topic(s): integrate ast-grep for search + refactor + editing
- Prior summary/context: prior compacted content: Archived 4 response topic(s): Compact Exchange stale-sidecar convergence; rollback-proof, se...

### Re: do [#astgrepstructural] — opus-5

Done. `tsift edit-intents` has a `structural_rewrite` intent kind, landed as `tsift@503c2df` and superproject `c2e7fe7b5`.

```json
{ "kind": "structural_rewrite", "file": "src/scan.rs",
  "pattern": "$A.unwrap()", "replacement": "$A.expect(\"scan\")" }
```

**Why this was worth a kind of its own rather than a flag on `ast-grep rewrite`.** `tsift ast-grep rewrite --apply` writes straight to the working tree — no reindex, no impact report, no temp worktree to fail in. That is the right shape for an exploratory codemod and the wrong shape for one you intend to land. Routing the same engine through the edit-intents planner means a pattern-driven codemod now earns exactly the proof a `rename_symbol` does: patch proposal, bounded diff preview, formatter policy, `expected_content_hash` conflict detection, `--verify` in a detached temp worktree with reindex plus `impact`, and batch rollback. None of that is re-specified for this kind; it is the same path.

**It is the only intent kind with no resolved symbol.** The other kinds all start from a name — an index row or a handle that already resolved to one symbol. A refactor usually starts from a *shape* instead, which is the gap the whole ast-grep surface exists to close. So the target is `file` + `pattern`, and `symbol`/`target_handle` are unused.

**Promoted across every executor at once** — Rust, Python, TypeScript, TSX, JavaScript, JSX, Markdown — because the selection and mutation logic are language-independent: the grammar is a parameter, not a code path. What *is* language-specific (parser validation, formatter policy) already lives in the executor contract. Language resolution goes through the contract `id` rather than a second parallel match, so a newly registered executor cannot advertise structural support it has no grammar for.

**Three things fail closed**, each with a test behind it:

- `pattern` is required by `structural_rewrite` and **refused on every other kind**. A pattern silently ignored by a symbol-resolved intent is not representable.
- Input *and* rewritten buffer are both reparsed with the executor grammar. The rewrite template is raw text, so that output reparse is the only thing standing between a template typo and a syntactically broken file.
- A pattern that matched nothing, and a template that reproduces every match, are **refusals** — not empty plans. The standalone `ast-grep rewrite` surface merely flags the latter `unchanged`; here a plan that cannot mutate must not be applyable.

**Verification.** 9 unit tests (multi-match, non-Rust executor, multibyte source, both degenerate refusals, output-reparse refusal, validation symmetry, and a drift guard asserting every registered executor resolves an ast-grep grammar) and 4 CLI integration tests (apply rewrites all 3 call sites and leaves the definition alone; dry run reports a diff and writes nothing; no-match refusal; `--verify` reaches `temp_applied_total=1` with the real tree byte-identical). `make check` exit 0, 0 failures, all 13 ran.

I mutation-checked rather than trusting green: dropping the no-match refusal, dropping the output reparse, and disabling the dispatch hook each failed the tests that claim to cover them — the last one failed all four integration tests, which is what tells me they reach ast-grep and are not just asserting the kind is accepted.

Live smoke on the installed 0.1.78 binary, not just the suite: `$A.unwrap()` → `$A.expect("smoke")` verified in a temp worktree with the real tree untouched, and all three refusals reproduce with their intended messages.

### Re: ast-grep as the core, and more languages — opus-5

> **Queue prompt:**
>
> do [#astgrepstructural]


> **Queue prompt:** Should we integrate ast-grep to support code search + refactor + editing code across many different languages? Should asg-grep be the core to convert code to ast or should we stick with the existing tsift ast bindings? I need to add more languages.

Taking the three parts separately, because they have different answers.

**Integrate it — done.** `tsift ast-grep search|rewrite|languages` shipped in 0.1.78, and as of this cycle it also reaches the semantic write path through `structural_rewrite` above.

**Do *not* make ast-grep the core.** Stick with the existing tsift AST bindings, for a concrete reason rather than a conservative one: `ast-grep-core` is a *matcher* over tree-sitter, not an indexer. It has no symbol extraction, no tag queries, no call edges, no persistence — everything `tsift index`, `graph`, and `explain` are built out of. Rebuilding those on top of it would mean writing the same extraction layer again with less control.

The usual argument for consolidating — "one parse, not two" — does not apply here. I checked during the ast-grep integration: `ast-grep-core` and `ast-grep-language` 0.44 resolve to **tree-sitter 0.26 and the same grammar crates tsift already compiles**. There is no duplicate tree-sitter, no duplicate grammar linkage, and no second parse to eliminate. They already share the substrate; the split is only in what each layer does with it.

So the right model is three surfaces over one grammar set, not one core: `search` answers *where is this concept* (index-backed), `edit-intents` answers *change this resolved symbol* (name-resolved), `ast-grep` answers *where does code have this shape* (pattern-matched). `structural_rewrite` is the seam where the third feeds the second.

**More languages splits into two jobs with very different costs — this is the part worth deciding before doing.**

`ast-grep-language` ships 28 grammars. tsift compiles 8 `lang-*` features today (rust, python, typescript, javascript, kotlin, zig, bash, markdown), and `zig` has no ast-grep grammar at all, so structural patterns cover 7.

- **Structural-only languages are nearly free.** One feature line plus three match arms in `tsift-astgrep/src/lang.rs` per language. That buys `ast-grep search`/`rewrite` and nothing else. The unclaimed set is roughly 19: go, java, c, cpp, c-sharp, dart, swift, ruby, php, scala, lua, elixir, haskell, nix, solidity, css, html, json, yaml, hcl.
- **Indexed languages are expensive.** `tsift-graph`/`tsift-search` need per-language tag queries and symbol extraction before a language can be searched, graphed, or made a semantic-edit executor. That is real per-language work, not a feature flag.

The cheap tier is worth taking soon and is directly useful to you: **go, cpp, c-sharp, dart, java** would give structural search and codemods across most of the lazily binding set, which is exactly the kind of "same change in nine repos" problem patterns are good at. I filed that as `#astgreplangs`.

The second tier — giving those languages semantic-edit executors so `structural_rewrite` and the symbol-resolved kinds reach them — is `#astgrepexec`, and it depends on the graph/search side, so it should not be started until the first lands.

I have not started either; they are queued in priority order, not gated. Say the word on `#astgreplangs` and I will take it — it is mostly mechanical, and the drift guard I added this cycle will catch any executor that gets registered without a grammar.

### Re: do [#astgreplangs] — opus-5

> **Queue prompt:**
>
> Please extract tsift:code-navigation section into a runbook. If AGENTS.md + CLAUDE.md are used and CLAUDE.md uses `@AGENTS.md`, please do not add redundant instructions.


Done — `tsift@3f79b39`, superproject `9720f0d41`. `tsift ast-grep languages` now lists **28**, up from 8.

Added: c, cpp, csharp, css, dart, elixir, go, haskell, hcl, html, java, json, lua, nix, php, ruby, scala, solidity, swift, yaml.

**They are a structural-only tier, and the feature wiring is where that's enforced.** Their `lang-*` features fan out to `tsift-astgrep` *only*, never to `tsift-graph`/`tsift-search`. A tree-sitter grammar is enough to match and rewrite a shape; indexing additionally needs per-language tag queries and symbol extraction. So these languages are matchable and rewritable but not indexed, not searchable, and have no semantic-edit executor — which also means `structural_rewrite` **refuses** them, since it needs an executor to reparse and validate its output. There's a test pinning exactly that boundary: Go rewrites fine through `ast-grep rewrite`, and `edit-intents` refuses it without writing. `lang-zig` is the mirror image — indexable, no ast-grep grammar.

**Probing each grammar live turned up four that don't behave like the rest.** I checked them individually rather than assuming the tier was uniform, and it's a good thing:

| Language | Quirk | Workaround |
|---|---|---|
| **C** | `foo($A)` matches nothing — tree-sitter reads it as a *declaration*, not a call | trailing semicolon: `foo($A);` |
| **CSS** | `color: $V` matches nothing | trailing semicolon: `color: $V;` |
| **Dart** | expression/statement patterns match nothing however spelled | declaration granularity only: `void main() { print($A); }` |
| **Solidity** | same as Dart | `function f() public { $$$B }` |

The other 16 match expression-level patterns directly.

**The Dart result is worth your attention**, because Dart was one of the five languages I put in the "first, for lazily-binding codemods" list. It does not deliver that: tree-sitter-dart can't parse a bare expression fragment, so there are no call-site codemods in Dart at all. Go, cpp, csharp, and java all work as intended. I added a companion test asserting those three still match call expressions directly, so a regression to Dart-like behaviour in the languages that actually motivated this work can't pass unnoticed.

**Drift guards.** The enum now has four parallel `match` arms across 28 variants, so I added two. One asserts every listed language is reachable from some file extension — a language added to `all()`/`from_name` but missed in `from_path` would be advertised and `--lang`-selectable while every directory walk silently skipped its files. One asserts every variant maps to the correspondingly named `SupportLang`, since a mismap is a wrong-grammar parse: worse than a refusal, because it silently under-matches instead of erroring. Both mutation-checked, and the pre-existing name round-trip test stays green under those mutations — which is what shows the new guards reach arms it couldn't.

`make check` exit 0, 1580 passed, 0 failed. CI was green on the preceding commit. Live smoke on the installed binary across go, java, cpp, csharp, elixir, lua, php, ruby, swift, scala, haskell, nix, hcl, html, json, and yaml.

`#astgrepexec` (executors for these languages) stays blocked where it was — it needs the graph/search tag-query work first, which is the same dependency that makes this tier structural-only.

### Re: do [#tsiftnavrunbook] — opus-5

Done — `tsift@1ed8f68`, superproject `64bd291d5`. Both parts of the ask, and they turned out to be one change.

**The duplication was worse than "repeated in both files."** tsift's own `CLAUDE.md` was `@AGENTS.md` followed by a verbatim copy of the ~4 KB block that `AGENTS.md` already carries — so a Claude Code session loaded it twice, and the second copy was the one that would drift. `tsift init` was the thing putting it there: it injected into `AGENTS.md`, then injected the same string into `CLAUDE.md` whenever that file existed, with no notion of one deferring to the other.

**De-duplication is deference-aware, not unconditional**, because the two ways a `CLAUDE.md` can defer need opposite handling:

| `CLAUDE.md` shape | What `init` does now |
|---|---|
| `@AGENTS.md` import | no section injected; an existing one is **removed** |
| symlink to `AGENTS.md` | **left alone** |
| standalone file | unchanged — still gets the section |

The symlink row is the one worth stating. `.exists()` follows symlinks, so the old code happily opened `CLAUDE.md`, saw the section (it was reading `AGENTS.md`), and rewrote through the link. Under a naive "defers ⇒ strip the duplicate" rule that becomes *delete the canonical copy* — the superproject, where `CLAUDE.md → AGENTS.md`, would have lost its Code Navigation block entirely. There's a test asserting the section still appears in `AGENTS.md` exactly once after init through a symlink, and it fails if `SameFile` is routed to the removal path.

**On extraction: the block is now a router, not a manual.** Session start, the envelope-over-raw-read substitutions, and the verification rule stay inline — those are what an agent needs before it has read anything. Budgets, `tsift workflow search`, `report.scale_guard` handling, the `tsift rewrite --run` path for harnesses without `PreToolUse` hooks, and the Codex/OpenCode install commands moved to a generated `runbooks/code-navigation.md`. The prompt-resident block roughly halves (4.2 KB → 2.1 KB).

**This does not reopen the hazard that kept the block self-contained.** The earlier decision — "avoid a runbook-only install that breaks standalone checkouts" — assumed the runbook was something a repo might or might not ship. It isn't anymore: `tsift init` **writes** it, under its own `<!-- tsift:code-navigation-runbook v=X.Y.Z -->` markers, so the pair ships together in every initialized checkout. And `tsift status` now reports `instructions: stale` when the runbook is missing or its marker version differs, even if the block marker is current — so a repo initialized by an older tsift is repaired by the `tsift init` / `tsift status --fix` it already recommends, with no new operator step. Text outside the runbook markers is preserved, which is how the superproject keeps its local `gh run watch` closeout convention in the same file.

**One latent bug found while building it.** `<!-- tsift:code-navigation-runbook` begins with the block's marker prefix, so `content.find(SECTION_MARKER_PREFIX)` matched the runbook's marker. The prefix now includes its trailing space (`<!-- tsift:code-navigation `), and a test pins that a runbook-marker-only file reads as `Missing`, not as a block.

**Verification.** `make check` exit 0, 1587 passed, 0 failed (was 1580). 8 new unit tests, 1 new CLI integration test, and a new SimWorld step + coverage requirement (`status/missing_runbook`) so the staleness edge runs in the seeded corpora, not just the named trace.

Mutation-checked rather than trusting green — six mutations, each caught by the test that claims to cover it:

- ignore the `@AGENTS.md` import → both dedup unit tests and the CLI test fail
- route symlinks through the removal path → the symlink test fails
- never write the runbook → 5 tests fail, including the two pre-existing version checks
- drop the runbook staleness downgrade → all 3 SimWorld tests fail with `missing runbook did not make instructions stale`
- delete `tsift workflow search` from the runbook body → the handoff-detail test fails
- drop the trailing space from the marker prefix → the collision test fails

Live on the installed binary, not just the suite: `tsift init` on the tsift repo reduced `CLAUDE.md` to a single line (`@AGENTS.md`), created the runbook, and reports `deferred` on re-run; on the superproject it reported `deferred` for the symlinked `CLAUDE.md` and left it untouched. Both repos report `instructions: current`, and a second `init` is a no-op in each.

CI is **in progress** on `1ed8f68` at the time of writing (run `30569395251`); the preceding commit was green, and the above is local evidence.

`#astgrepexec` is unchanged — still blocked on per-language tag queries and symbol extraction in `tsift-graph`/`tsift-search`, which is the same dependency that makes the 20 new grammars a structural-only tier.

### Re: do [#crosslanguageconformance] — opus-5

Done — `tsift@4a93d7f`, superproject `56a018138`. Two suites, and they found two defects before I finished writing them.

**The gap wasn't missing tests, it was tests that couldn't miss anything.** Both language tiers already had per-language coverage: `test_extract_rust_symbols`, `test_extract_python_symbols`, a `grammar_quirk_tests` module. Every one of them proves that the language *someone remembered to write a test for* works. None of them notices a new grammar arriving with no test, and none checked the invariants that must hold identically everywhere — span validity, ordering, empty input, broken input.

So the unit of coverage is now the **table**, not the language:

| Suite | Rows | Guard |
|---|---|---|
| `tsift-astgrep/tests/conformance.rs` | 28 (one per `AstGrepLang`) | fixture set **==** `AstGrepLang::all()` |
| `tsift-graph/tests/conformance.rs` | 10 (one per `Lang`) | fixture set **==** `Lang::all()` |

Adding a grammar without a fixture row now fails. Grammar quirks are row data (`granularity`, `known_non_matching`), not prose — so when an ast-grep bump makes Dart parse expression fragments, the suite fails instead of leaving that table in the spec quietly wrong.

**Three things stop it reporting green over nothing**, which is the specific way conformance suites rot: every row must declare `expected_matches > 0`; the runner sums the matches *the library actually returned* and asserts they equal what the table declares (not a loop counter); and an empty table is itself a failure. That last one was a live hole — `tsift-astgrep`'s own `default` feature set is empty, so `cargo test -p tsift-astgrep` compiled zero grammars and the exhaustiveness guard passed vacuously. It now says so and exits non-zero.

**Defect 1 — an unparseable pattern aborted the process, on every language.**

```
$ tsift ast-grep search 'foo(1); foo(2);' --lang rust
thread 'main' panicked at ast-grep-core-0.44.1/src/matcher/pattern.rs:356:30:
called `Result::unwrap()` on an `Err` value: MultipleNode("foo(1); foo(2);")
```

`ast-grep-core`'s `&str`-as-matcher path builds the pattern with `Pattern::new`, which `unwrap()`s the parse. A pattern that is *merely invalid for the selected grammar* — two statements, or `"a": $V` in JSON — killed the CLI. The pattern comes straight from the user, so this is a crash on ordinary bad input, and it was not JSON-specific: rust, python, go, java and bash all did it. `search_source`/`rewrite_source` now compile with `Pattern::try_new` up front and surface the grammar's own message. Same four languages now exit 1 with `invalid structural pattern for rust: Multiple AST nodes are detected...`.

**Defect 2 — Kotlin call edges were silently always empty.** `call_query()` named `simple_identifier`, a node type from the older `tree-sitter-kotlin` grammar; `tree-sitter-kotlin-ng` calls it `identifier`. `Query::new` failed, the indexer downgraded that to a warning, and every Kotlin file produced zero call edges. `graph --callers`, `graph --callees`, and `explain` returned nothing for Kotlin and nothing ever failed. Compiling the call query is the invariant that caught it — no per-language test did, because none of them called `call_query` for Kotlin at all.

Live on the installed binary, on a real indexed Kotlin file:

```
$ tsift graph main --callees
Callees of `main`:
  helper (Main.kt:2)
  method (Main.kt:3)
```

Both direct and `obj.method()` navigation calls now resolve.

**Two pattern quirks also came out of the sweep** and are in the spec table: HCL has no expression statements, so a call only matches as `$K = foo($A)`; JSON needs both sides metavariable-shaped (`$K: $V`), since a literal key with a metavariable value parses to two nodes — which is exactly the input that used to panic.

**Verification.** `make check` exit 0, 1595 passed, 0 failed (was 1587). Seven mutations, each caught by the test that claims to cover it:

- delete one fixture row → both exhaustiveness guards fail
- revert the Kotlin query to `simple_identifier` → the graph conformance suite *and* the new behavioural test fail
- revert `compile_pattern` to `Pattern::new` → `an_unparseable_pattern_is_refused_on_every_language` panics rather than fails, which is the defect reproducing
- reverse the sort in `extract_symbols` → `rust: symbols are not line-ordered`
- drop one extension from `from_path` → the structural contract fails on that row
- truncate the rewrite edit list to one → `rust: rewrote 1 match(es), expected 2`
- widen a body span by one byte → `rust: symbol main body ends past the symbol`

CI on the preceding commit (`1ed8f68`, the runbook split) came back **green**; the conformance run is still in progress at the time of writing.

I filed the one real gap left: the tables drive `search_source`/`rewrite_source` over in-memory buffers, but the CLI's `scan`/`codemod` path — extension dispatch across a real directory, `--apply` budgets, preview byte-identity — is still only covered for a handful of languages. Same fixture table, written to a temp tree.

### Re: do [#extendcrosslanguage] — opus-5

> **Queue prompt:**
>
> do [#extendcrosslanguage]


Done — `tsift@942656e`, superproject `76926dcf5`. Extending the table to the file-scan path found a third defect, which is what I'd hoped for: `search_source` takes a language as an argument, and the CLI doesn't — it walks a tree and picks a grammar per extension, so the two paths can disagree.

**A mixed-language tree could not be scanned with a pattern that wasn't valid everywhere.** A pattern is only ever valid for *some* grammars — I measured it across the table, and 11 of the 12 patterns in it are refused by at least one of the 28:

| Pattern | Refused by |
|---|---|
| `foo($A)` | — (the only universal one) |
| `foo($A);` | python, bash, elixir, go, lua, nix, ruby, scala |
| `color: $V;` | 10 languages |
| `void main() { print($A); }` | 11 languages |

`scan` propagated that per-file error, so **one** `.py` file in the tree made a C-style pattern return nothing at all. Before the `Pattern::try_new` fix in the previous cycle it didn't error, it panicked the process. That's not an edge case — a mixed repository is the normal case for a tool whose whole point is cross-repo codemods.

A file whose language can't compile the pattern is now skipped and counted, and **the skip is visible** rather than silent:

```
$ tsift ast-grep search 'foo($A);'
a.rs:2:4: foo(1);
1 match(es) in 1 of 3 scanned file(s) [2 file(s) skipped: pattern does not parse as go, python]
```

`files_skipped_pattern_unsupported` and `pattern_unsupported_langs` are in the JSON envelope too. **When every scanned file was skipped that way, the scan is an error, not an empty result** — otherwise a typo'd pattern reports a confident `0 match(es)`, which is the worst possible answer. Exit 1 with `no scanned language could compile the pattern ... (tried: go, python, rust)`.

**The file-scan rows drive the same 28-row table through the walk**, so the two paths can't drift: extension dispatch must resolve each fixture to its *own* language with the same match count the buffer search produced; non-source decoys (`notes.txt`, a nested `.log`) are never parsed; preview leaves every file byte-identical; and `--apply` on one named file changes that file and leaves all 27 other languages and both decoys untouched.

**Verification.** `make check` exit 0, 1600 passed, 0 failed (was 1595). CI is green on both preceding commits (`4a93d7f`, `e7412d0`). Live on the installed binary: the three cases above, plus a universally-valid pattern reporting zero skips.

Seven mutations, each caught:

- propagate the per-file error again → 3 tests fail, which is the pre-fix behaviour reproducing
- swallow the all-refused case as an empty result → the confident-zero test fails
- preview writes to disk → `preview wrote to ... — the whole contract is that it does not`
- apply writes the original source back → the apply test fails
- walk forces one grammar instead of extension dispatch → 3 tests fail
- drop the extension filter in the walk → `rust: notes.txt was parsed`
- rename the decoy to a real source extension → `rust: notes.rs was parsed`

One process note: I reverted `conformance.rs` with `git checkout` during the mutation sweep instead of restoring from the scratch backup I'd been using for every other file, which discarded the file-scan section I'd just written. Re-added it and re-verified. The mutation that prompted it was also a no-op — its replacement string never matched, so it "passed" without testing anything; I re-ran it properly and it does fail.

Queue after this: `#astgrepexec` (still blocked on per-language tag queries and symbol extraction in `tsift-graph`/`tsift-search`) and `release + publish`.

### Re: do [#astgrepexec] — opus-5

Done — `tsift@bff3f7a`, superproject `8e200a09e`. **Kotlin and Bash are semantic-edit executors now.** The recorded blocker was only half true, and checking it was most of the work.

**The item said this was blocked on per-language tag queries and symbol extraction. For Kotlin and Bash it wasn't** — both have had graph symbol extraction the whole time, and both have ast-grep grammars. The blocker is real for the 20 structural-only languages added by `#astgreplangs`; it never applied to these two.

**They're registered as a structural-only family.** `structural_rewrite` is dispatched ahead of the per-family split and needs only a grammar to match with and a grammar to reparse the result — so a language doesn't need per-kind rewriting to be a useful executor. Kotlin and Bash get the whole planner contract for pattern-driven codemods: patch proposal, bounded diff preview, `expected_content_hash` conflict detection, `--verify` in a detached temp worktree, batch rollback. Their recognized-intent set is `structural_rewrite` alone; `rename_symbol`, `replace_function_body`, and `insert_import` stay unrecognized because their implementations are genuinely language-specific and no family implements them for these two.

**Registering them exposed a fall-through that mattered more than the feature.** The family split routes anything that is neither markdown nor script to the **Rust** implementations. So a Kotlin `rename_symbol` went to `replace_rust_identifier`, and on a trivial case it produced *plausible output*:

```
"status":"applied", "language":"kotlin",
"diff":"-fun main() {\n+fun main2() {",
"message":"applied rename_symbol intent through the Kotlin semantic edit executor"
```

That is Rust identifier rules rewriting a Kotlin file while claiming to be the Kotlin executor. Worse than a refusal, because it looks right. The catch-all further down even hardcodes "not supported by the **Rust** executor yet", so the one case that did refuse named the wrong language. An executor that does not recognize a kind is now refused *before* the split, naming the executor and its actual supported set.

**Verification.** `make check` exit 0, 1601 passed, 0 failed. CI green on the preceding commit (`942656e`). Live on the installed binary:

```
$ tsift edit-intents --apply  # structural_rewrite on Main.kt
  eintent-ee7d3077fa applied structural_rewrite ... through the Kotlin semantic edit executor
  fun main() { bar(1); bar(2) }

$ tsift edit-intents --apply  # rename_symbol on the same file
  Error: ... "rename_symbol" is not supported by the Kotlin executor (supported: structural_rewrite)
  # file unchanged
```

Three mutations, each caught: removing the Kotlin contract, giving Kotlin the full Rust kind set, and deleting the fall-through guard — that last one failed with the silent Rust-rules rewrite above, which is the whole reason the guard exists.

One correction worth recording: my first attempt at that third mutation edited the wrong occurrence — there are two `!executor.recognized_intents().contains(&kind)` sites and the naive replace hit the older one in `semantic_edit_kind_apply_supported`, where it is benign. It "passed", which would have read as the guard being untested. Retargeted at the real guard, it fails. Same class of mistake as the no-op mutation last cycle; I'm now matching on the surrounding block, not the predicate line.

I also added a contract-exhaustiveness guard: every registered executor must appear in the contract test, so the next language added can't arrive with no coverage.

**What's left is genuinely blocked**, and I filed it that way: the 20 structural-only languages need per-language tag queries and symbol extraction in `tsift-graph`/`tsift-search` first, because `parse_semantic_edit_source` validates through `executor.graph_lang()`. Once a language has graph support, registering it is a contract row plus a conformance fixture — it unblocks one language at a time rather than as one big step.

Queue after this: `release + publish`.
### Re: do [#givestructuralast] — opus-5

> **Queue prompt:**
>
> do [#givestructuralast]


Done — `tsift@cd95daa`, superproject `b526d61c7`. All 20 are executors. **The blocker I recorded was wrong, and it was wrong in a way worth naming.**

The item said each language needed per-language tag queries and symbol extraction in `tsift-graph`/`tsift-search` first, *because* `parse_semantic_edit_source` validates through `executor.graph_lang()`. The second half of that sentence is true and the first half does not follow from it. Reparsing a rewritten buffer needs a **parser**. Tag queries and symbol extraction are what *indexing* needs. Routing the reparse through `graph::Lang` made a navigation capability a precondition for a write capability that never depended on it — and because the coupling was in the type, the obstacle read as a property of the languages rather than of one field.

`graph_lang` is now `Option<graph::Lang>`, and the reparse grammar resolves in two steps:

| Executor | Reparse grammar |
|---|---|
| has a `tsift-graph` binding | that binding's grammar |
| structural-only | the ast-grep grammar its pattern matched against |

`AstGrepLang::tree_sitter_language()` exposes the second. An executor with neither is a registration bug, refused by name rather than parsed with some other language's rules. **29 registered executors, up from 9**, each with the whole planner contract — patch proposal, bounded diff preview, `expected_content_hash` conflict detection, `--verify` in a detached temp worktree, batch rollback.

**What this does not change is the tier.** These languages are still unindexed, unsearchable, and ungraphable; `structural_rewrite` is still their entire recognized set, and the symbol-resolved kinds are still refused before the family split — which is the guard that keeps a Go `rename_symbol` from being rewritten by Rust identifier rules. Indexing buys navigation, not rewriting. That is the distinction the old item collapsed.

**The load-bearing fact I found while building it, which no test was holding.** My first mutation — delete the `graph_lang` branch so *everything* reparses with the ast-grep grammar — **passed**. It shouldn't have been able to; the two grammars are supposed to be able to differ. So I measured them, and for all nine indexed executors they are byte-for-byte the same grammar: same ABI, same node-kind table. Even Kotlin, where ast-grep's feature is spelled `tree-sitter-kotlin` and `tsift-graph` uses `tree-sitter-kotlin-ng` — cargo resolves both to the `-ng` grammar. That is exactly the pair that produced the silent zero-call-edge defect two cycles ago, and nothing was asserting they still agree. A grammar bump that splits them would have the planner validating output against a grammar the matcher never used and the indexer will not use either, silently. There is now a test that compares every node-kind id across both and fails at that moment.

**Coverage is one conformance fixture per registered executor, exhaustive in both directions** — an executor with no fixture is registered but never exercised; a fixture with no executor has outlived what it covered. Each row drives the real planner path for all 29 languages, and the suite compares the sum of replacements *the planner returned* against the sum the table declares, so a runner counting its own iterations cannot report green over a table that rewrote nothing. Grammar quirks stay row data: C and CSS need the statement terminator, Dart and Solidity match only whole declarations, HCL only as an attribute, JSON needs both sides metavariable-shaped.

The one CLI test that pinned the old boundary — "Go rewrites via `ast-grep` but is refused by `edit-intents`" — is inverted rather than deleted, and now also asserts the apply actually lands and that a symbol-resolved kind still fails without writing.

**Verification.** `make check` exit 0, 1608 passed, 0 failed (was 1601). Eight mutations, each caught by the test that claims to cover it:

- drop the `graph_lang` branch → **survived**, which is the finding above; the new grammar-agreement guard now catches the case it was hiding
- remove the Go contract row → 4 tests fail
- give Go the full Rust kind set → the contract test and the structural-refusal test fail
- structural executors reparse with the Rust grammar → 3 tests fail
- truncate the fixture table to the original nine → 4 tests fail
- no-match becomes an empty plan instead of a refusal → 2 tests fail
- the grammar-agreement guard compares against Rust instead of the executor → it fails
- `AstGrepLang::tree_sitter_language` returns one constant grammar → the pairwise-distinct guard fails

Live on the installed binary, not just the suite: Java and CSS codemods applied through their named executors; a Go dry run reports `apply_supported=true` and writes nothing; `--verify` on Go reaches `temp_applied_total=1`, `reindexed=true`, `changed_symbols=0` (right — Go has no index) with the real tree untouched.

One honest gap. The executor-level refusal of `rename_symbol` is asserted per language in the unit suite, but it is currently **unreachable end to end**: an unindexed language has no symbol to resolve, so the index layer refuses first. It is a guard for the moment one of these languages gains graph support, not a path in use today.

CI run `30577467047` is queued on `cd95daa` at the time of writing; the two preceding commits are green and the above is local evidence.

Queue after this: `release + publish`.
<!-- agent:boundary:2640f259 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
- 🚧 do [#givestructuralast]
<!-- /agent:queue -->
<!--
https://github.com/chopratejas/headroom
-->

## Backlog

<!-- agent:backlog priority queue -->
- [x] 🚧 [#givestructuralast] Give the 20 structural-only ast-grep languages (go, cpp, csharp, dart, java, swift, ruby, php, scala, lua, elixir, haskell, nix, solidity, css, html, json, yaml, hcl, c) semantic-edit executors. Genuinely blocked: each needs per-language tag queries + symbol extraction in tsift-graph/tsift-search first, because parse_semantic_edit_source validates through executor.graph_lang(). Unblocks one language at a time as its graph support lands — the executor registration itself is then a contract row plus a conformance fixture
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
<!-- /agent:review -->

<!-- agent:icebox -->
- [ ] [#g5kw] What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
- [ ] [#4z0w] Can we increase max-hop caps?
- [ ] [#cg52] Could FalkorDB help with performance?
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->

<!-- agent:notes -->
• Ran tsift rewrite --run 'git log -5 --oneline --decorate'
└ tsift rewrite: no rewrite: no supported tsift rewrite matched this command; `--run` executes only rewritten commands; run the original command directly if intended

#spec-test-build-install-commit-push
---
route duplicate prompt residue guard failed: failed to parse components: unclosed component: `agent:notes` without matching close
<!-- /agent:notes -->
