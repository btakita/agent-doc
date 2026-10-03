---
agent_doc_session: tsift-v0.1
resume:
  claude: 03da93b9-58a8-4aa3-b530-ca5405b2ea0b
  codex: 01a0ca63-49bb-7040-a638-a80239e1503e
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
  '#subagent': 'run in a subagent'
  '#subagents': 'run the remaining items in subagents'
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20261002-042733.md`*

Compacted content:
- Archived 4 response topic(s): #ciwalflakeregress; Response; release + publish; 1 more
- Prior summary/context: prior compacted content: Archived 16 response topic(s): fpe.md runaway tsift task; agent-doc commit and push check; t...

### Re: recovered response

Published successfully.

- `main` and `v0.1.101` point to `7b18e0a`.
- [Release workflow](https://github.com/btakita/tsift/actions/runs/36965897098): passed.
- [GitHub release](https://github.com/btakita/tsift/releases/tag/v0.1.101): published with Linux, Windows, and macOS assets.
- crates.io `tsift-cli`: `0.1.101`.
- npm `opencode-tsift`: `0.1.101`.
- [Main CI](https://github.com/btakita/tsift/actions/runs/36965891592): passed.
- The graph-refresh guard rejects oversized federated work before materialization.

I preserved an unstaged local edit in [VERSIONS.md](/home/brian/work/btakita/agent-loop/src/tsift/VERSIONS.md) that moves the guard note under “Unreleased”; it was not included in the release.

Agent-doc’s response is committed, although its post-compaction `session-check` still falsely interprets the generated “Prior summary/context” line as an unanswered prompt. No force-write or duplicate response was used.

### Re: Should we release + publish? — opus-5.5 · 2026-10-03T02:08-04:00

> **Queue prompt:** Should we release + publish the

The question was cut off. I'm assuming it means whatever tsift has that isn't released yet.

**Recommendation: yes, release it as 0.1.102, but wait until CI on that commit passes. I haven't released anything this turn.**

What isn't released yet:
- `origin/main` is one commit ahead of `v0.1.101`: `c902e41 feat(graph): index C++ for search and graph (#cppindex)`. Another session pushed it at 01:59. Its VERSIONS.md entry is already under "Unreleased".
- [CI 37101600385](https://github.com/btakita/tsift/actions/runs/37101600385) on `c902e41` is still running: the Convex acceptance job passed and `clippy + test` is in progress. A tag pushed now would publish code that hasn't been tested yet.
- The session that wrote `#cppindex` may already plan to release it. Releasing from here at the same moment would risk two releases racing each other.

A local file needs cleanup before any release from this checkout:
- `src/tsift` is checked out at `7b18e0a` (`v0.1.101`), one commit behind `origin/main`.
- Its uncommitted VERSIONS.md edit is out of date. It moves the "fail fast before swap thrash" guard note under "Unreleased", but that guard was released in `v0.1.101`: the tag contains both the `FEDERATED_GRAPH_SYMBOL_LIMIT` code and the note.
- This edit should be discarded (`git checkout -- VERSIONS.md`) before the next release. If it were kept, the changelog would say the guard is unreleased, and the edit would conflict when pulling `c902e41`. I left it as is because this turn didn't create it.

If you meant something other than the C++ indexing commit, finish the sentence. Otherwise reply `go`, and the next cycle will discard the stale edit, pull to `c902e41`, wait for CI to pass, then bump to 0.1.102, tag and push. Pushing the tag publishes to GitHub Releases, crates.io and npm.

### Re: #releasepublishbindings — opus-5.5 · 2026-10-03T03:05-04:00

> **Queue prompt:** do [#releasepublishbindings]

Released **tsift v0.1.102**, which adds C++ indexing (`#cppindex`, `c902e41`).

- First I discarded the stale local VERSIONS.md edit. It listed the swap-thrash guard as unreleased, but that guard already shipped in 0.1.101. Then I fast-forwarded to `c902e41`, whose [CI](https://github.com/btakita/tsift/actions/runs/37101600385) had passed.
- The version bump touched the same files as the 0.1.101 release: workspace and package `Cargo.toml`, `Cargo.lock`, skill and runbook version stamps, the OpenCode command files and `package.json`, `specs/search-navigation.md`, and a fixture's projection version. No `0.1.101` remains outside the VERSIONS.md history.
- `make check` exit 0: **1879 passed, 0 failed**. The npm pack check produced `opencode-tsift-0.1.102.tgz`.
- `33bc48f chore: release v0.1.102` was pushed to `main`, along with tag `v0.1.102`.
- [Release workflow](https://github.com/btakita/tsift/actions/runs/37102150648): all 7 jobs passed.
  - [GitHub release](https://github.com/btakita/tsift/releases/tag/v0.1.102): published with Linux, Windows and macOS (arm64) assets.
  - crates.io `tsift-cli`: `0.1.102`.
  - npm `opencode-tsift`: `0.1.102`.

Since the release, other work has landed on `main` under "Unreleased". Contract indexing is in (`8022f34`, from another session), and C indexing (`d4bb5bf`, from a subagent here) is pushed but its queue item isn't closed yet.

**Queue items dispatched to subagents.** Each is claimed in the queue and has its own worktree under `~/work/btakita/worktrees/`. Each pushes to `main` without force.
- `#subagent: Add C bindings`: **done**, `d4bb5bf feat(graph): index C for search and graph (#cindex)`. `make check` passed, and it is installed. `.h` files stay parsed as C++, since the C++ grammar also handles C headers but the C grammar fails on C++ headers.
- `#subagent: Add Odin bindings`: running.
- `#subagent: Add Jai bindings`: running. Whether a usable tree-sitter grammar exists is the main risk.
- `#subagent: [#sdktsiftedges]`: running. It is building `graph-db` edge-file import plus loading haiven-sdk's `trace.json`.
- New backlog items `#xsdcontract` and `#wsdlcontract`: running in **one** subagent, XSD first and then WSDL. WSDL embeds XSD, and both edit `contract.rs`.

**Contract formats.** The `8022f34` contract projection already covers OpenAPI (including Swagger 2), AsyncAPI v2 and v3, and JSON Schema. Other JSON and YAML files are searchable as text but produce no symbols. For XML, only the formats that describe an API or a build are worth indexing: XSD, WSDL, and possibly project files like `.csproj` and `pom.xml`.

### Re: C bindings done; XSD/WSDL already in flight — opus-5.5 · 2026-10-03T02:45-04:00

> **Queue prompt:** #subagent: Add C bindings

**Done.** The subagent pushed `d4bb5bf feat(graph): index C for search and graph (#cindex)` to `main`.
- `.c` files now contribute these symbols to search and graph:
  - structs, unions, enums and their enumerators;
  - typedefs, including pointer and function-pointer typedefs;
  - function definitions and prototypes;
  - fields;
  - file-scope globals, including those inside `#if` blocks;
  - macros that take a value or parameters.
- Call edges cover direct calls and calls through a function-pointer member.
- The symbol-extent helper is now shared by C and C++ (`c_family_declaration_for`), and C++ behaves the same as before.
- `.h` stays parsed as C++: the C++ grammar handles C headers, but the C grammar fails on classes and namespaces. If a build has C but not C++, `.h` falls back to C.
- `make check` exit 0, about 1,889 tests passed with none failing. Installed with `cargo install --path . --locked --force`. A smoke test found definitions and prototypes, and `graph --callees` resolved.

`#xsdcontract` and `#wsdlcontract` reached the queue when the deferred backlog write landed. The XML subagent I already dispatched is working on both, in that order, in `~/work/btakita/worktrees/tsift-xml`. I've claimed both heads for it so this loop doesn't run them a second time. The Odin, Jai and `#sdktsiftedges` subagents are still running.

### Re: Odin bindings done — opus-5.5 · 2026-10-03T02:48-04:00

> **Queue prompt:** #subagent: Add Odin bindings

**Done.** The subagent pushed `db968c2 feat(graph): index Odin for search and graph (#odinindex)` to `main`, rebased onto the C commit without force.
- **Grammar:** the `tree-sitter-odin` 1.3.0 crate from crates.io (published by tree-sitter-grammars). It works with the workspace's tree-sitter 0.26.9, so nothing had to be vendored.
- **Symbols:**
  - procedures, including proc groups, those inside top-level `when`, and `foreign` prototypes;
  - structs, unions and enums, plus enum members and `bit_field`s;
  - fields, constants, type aliases and package-level variables.
  - A constant whose value is a type counts as a type alias.
- **Call edges:** `f()`, `pkg.f()` and `obj->f()`. `import` and `foreign import` count as imports for impact analysis.
- **Name filter:** the Odin grammar doesn't mark which identifier is the declaration name. A new filter keeps only the identifier before `::`, `:` or `:=`, so values, proc-group members and locals are not symbols. The symbol test fails when the filter is disabled.
- **Rename:** not available for Odin. ast-grep has no Odin grammar, so Odin joins the indexed-but-not-structural group with Zig and GDScript.
- **Verification:** `make check` exit 0 both before and after the rebase. Installed, and a smoke test passed: search found the proc group, and `graph add --callers` returned `main` and `inner`.

The Jai, `#sdktsiftedges` and XSD/WSDL subagents are still running, and their heads stay claimed.

### Re: Jai bindings done; opt-in question — opus-5.5 · 2026-10-03T02:54-04:00

> **Queue prompt:** #subagent: Add Jai bindings

**Done.** The subagent pushed `6b8b435 feat(graph): index Jai for search and graph (#jaiindex)` to `main`, rebased onto C and Odin without force. `make check` exit 0: 1894 passed, 0 failed.
- **Grammar:** crates.io has no Jai grammar, so the subagent vendored [constantitus/tree-sitter-jai](https://github.com/constantitus/tree-sitter-jai) `96440b0`. Its MIT-0 license is compatible with tsift's MIT.
  - The files live under `packages/tsift-graph/vendor/tree-sitter-jai/`, and a new `build.rs` compiles `parser.c` and `scanner.c` with `cc` when `lang-jai` is on.
  - **Cost:** `parser.c` is **29 MB**, which adds about 872k lines to the repo and about 1.2 MB gzipped to the crate. It also adds about a minute to a cold build.
- **Symbols:** procedures (including nested ones), structs, unions, `enum` and `enum_flags` with their members, named `#import`s, `#type` aliases, file-scope constants and variables, and fields.
  - A plain `Handle :: u32` is reported as a `const`, because the grammar parses it like any constant.
- **Call edges:** `f()` and `Module.f()`. `#import` and `#load` count as imports for impact analysis.
- **Not supported:** rename and structural rewrite, because ast-grep has no Jai grammar.
- **Not tested on real Jai code:** there is none on this machine, and the grammar's own test file parses with 3 error nodes.

**Making all languages opt-in (asked in the console).** I recommend against making every language opt-in, and only Jai needs it. Waiting on the operator's decision:
- With everything off by default, `cargo install tsift` would produce a binary that can't index anything. The mainstream languages should stay on by default.
- A Cargo feature only controls what gets *compiled*. With `lang-jai` off, the 29 MB `parser.c` still ships in the repo and in every crates.io `tsift` download. The real fix is moving the grammar into its own crate (for example `tsift-grammar-jai`), which `tsift-graph` pulls in only under `lang-jai`.
- `release.yml` builds with the default features. If anything becomes opt-in, the release build should use `--features all-languages` so the prebuilt binaries still include every language.
- Proposed tiers:
  - **On by default:** the mainstream languages, plus zig, odin and gdscript, whose grammars are small and published on crates.io.
  - **Off by default:** `lang-jai`, moved into its own grammar crate. The release workflow would publish that crate before `tsift`.
  - **Prebuilt release binaries:** built with `all-languages`.

The `#sdktsiftedges` and XSD/WSDL subagents are still running.

### Re: XSD + WSDL contracts done; #jaicrate dispatched — opus-5.5 · 2026-10-03T03:03-04:00

> **Queue prompt:** do [#xsdcontract]

> **Queue prompt:** do [#wsdlcontract]

**Done.** One subagent pushed both commits to `main` without force. `make check` exit 0 at each commit by itself, with 68 result lines, no failures and no clippy warnings.
- `1316b66 feat(graph): index XSD as an API contract (#xsdcontract)`, rebased over the Odin and Jai commits.
- `385e043 feat(graph): index WSDL as an API contract (#wsdlcontract)`.

**How it works:**
- **Parser:** `tree-sitter-xml` 0.7, not `quick-xml`. Every indexed language must supply a tree-sitter grammar, which chunking, complexity scoring, semantic-edit reparsing and the conformance suite all rely on. Sharing that tree also keeps symbol spans identical to the chunker's, as with the JSON and YAML code.
- **Layout:** a shared XML layer, `tsift-graph/src/xml.rs`, resolves namespaces by scope, so `xs:`, `xsd:` and a default namespace are all handled without hardcoded prefixes. The XSD and WSDL code is in `xml_contract.rs`. A WSDL's embedded `<types>` schemas go through the XSD code.
- **XSD:** components become `schema` symbols, with the XSD element name kept as `node_kind`, so an element and a type with the same name stay distinct. Child elements and attributes become `Owner.child` properties. `type=`, `ref=`, `base=`, `substitutionGroup=`, `itemType=` and `memberTypes=` are references. Built-in `xs:` types and undeclared prefixes produce no edge.
- **WSDL:** services, ports (`Service.port`, kind `endpoint`), portTypes and interfaces (kind `interface`), operations, messages and parts, and bindings become symbols. `binding`, `service` and `endpoint` can now be callers.
- **Features:** `lang-xsd` and `lang-wsdl`, where WSDL requires XSD.
- **`.xml` is not indexed.** Unlike JSON, XML contracts have their own extensions. Plain `.xml` is mostly IDE state, build files and generated output. A test asserts that `pom.xml` maps to no language.
- **Smoke test after install:**
  - `graph Address --callers` returns `Customer` through `type=` and `UsAddress` through `base=`.
  - `TradePriceRequest --callers` returns both WSDL messages.
  - The port shows up as a caller of the binding.

**`#jaicrate` is dispatched and claimed** (subagent `tsift-jaicrate`, worktree `~/work/btakita/worktrees/tsift-jaicrate`):
- **New crate:** the vendored grammar moves into a new `tsift-tree-sitter-jai` crate. It exposes the standard grammar-crate API (`LANGUAGE`, `NODE_TYPES`), so switching to an upstream `tree-sitter-jai` crate later is a one-line change.
- **Opt-in:** `lang-jai` is no longer a default feature, and CI still runs the Jai tests.
- **Release:** binaries are built with `--features all-languages`, and the new crate is published before `tsift-graph`.

Upstream: opening an issue on constantitus/tree-sitter-jai, to offer the `scanner.c` fix and offer to publish `tree-sitter-jai` with them as owner, is waiting on the operator's go-ahead.

`#sdktsiftedges` is still running. The exchange is long (132 lines), but the queue is active, so I'm not compacting.

### Re: #sdktsiftedges done — opus-5.5 · 2026-10-03T03:14-04:00

> **Queue prompt:** #subagent: [#sdktsiftedges]

**Done.** The subagent pushed two commits to `main` without force, rebased over C, Odin, Jai, XSD and WSDL. `make check` exit 0, and the build is installed.
- `5bc504c feat(graph-db): import external edge files (#sdktsiftedges)`
- `c05c7af fix(graph-db): match trace REST operations by METHOD /path first, then operationId`. haiven's OpenAPI files have no `operationId`, so matches went from 0/195 to 139/195.

**Command.** `tsift graph-db [--path P] [--scope S] import <FILE> [--source ID] [--format auto|edges|haiven-trace]`, plus `import --list` and `import --remove (--source ID | FILE)`.
- **Format (`tsift-graph-edges/v1`):** `{format?, source?, nodes?: [{id, kind?, label?, join?, properties?}], edges: [{from, to, kind, properties?}]}`.
- **Joins:** a node's `join` links it to an existing indexed node:
  - by name, optionally narrowed by `languages` or `path`, with fallback `aliases`;
  - by `path` + `line` (the declaration site);
  - by `path` alone, for a file node.

  A name that matches 2 to 8 nodes links to all of them; more than 8 links to none.
- **Unmatched endpoints:** an undeclared endpoint that matches nothing becomes an `external` node. These are counted, not treated as errors.
- **Storage and re-import:** imports are stored under `graph-imports/` and re-applied on every refresh, so they survive `refresh --rebuild`. Re-importing the same `source` replaces that source's rows.
- **trace.json:** recognised automatically. The sibling `model.json` adds `carried_by` links (contract → event or request) and `used_by` links (REST component → operation).
- **Docs:** specified in `specs/graph.md` under "External Edge Import", with updates to `SPEC.md`, the README and VERSIONS.

**Real import.** `trace.json` exists only on the unmerged haiven-sdk branch `feat/codegen-trace` (PR #62). The subagent read it there and did not modify it. It then re-indexed the `haiven-contracts` scope to pick up the 3,216 JSON symbols and imported the trace. Only `.tsift/` state was written.
- **Loaded:** 1,640 nodes and 1,762 edges.
- **Matched:** 309 nodes matched indexed ones, 81 of them to more than one.
- **Unmatched:** 1,307, mostly SDK declaration sites whose files aren't in this scope. 0 `external` endpoints.
- **Blast radius** from `ChatMessageV0` with `neighborhood --depth 2`:
  - its declarations in TypeScript, C#, C++ and GDScript;
  - the events that carry it: `chat.message`, `chat.message.updated` and `chat.message.deleted`;
  - its nested record type;
  - the conformance scenario `gaming/v0#an-event-carries-no-correlation`.

**Follow-ups filed:**
- `#graphrefreshperf`: a root-scope refresh over the SDK tree ran over 13 minutes, about half of it in `link_ast_navigation_edges`. This slowness already existed and is what blocked the root-graph import.
- `#neighborhoodlimit0`: the default output budget hid 11 declarations, and `--limit 0` doesn't lift it.
- `#sdktraceroot` [recommended]: after the refresh fix, import into the root graph and investigate the unmatched trace nodes.

Worth knowing: the trace links only 1 of 228 operations to a scenario. That's a gap in haiven-sdk's trace data, not in tsift.

`#jaicrate` is still running. The exchange is long (163 lines), but the queue is active, so I'm not compacting.

### Re: #graphrefreshperf + #neighborhoodlimit0 dispatched — opus-5.5 · 2026-10-03T03:17-04:00

> **Queue prompt:** do [#graphrefreshperf]

These two items are independent, so each went to its own subagent, claimed in the queue, with its own worktree based on `c05c7af`:
- **`#graphrefreshperf`** (subagent `tsift-refreshperf`, `~/work/btakita/worktrees/tsift-refreshperf`):
  - Reproduce on a scratch copy of the haiven-sdk `sdk-ruff` tree; the original stays read-only because it belongs to another session.
  - Profile `link_ast_navigation_edges` and fix the root cause.
  - Prove the edge set is unchanged, add a perf-gate case, and report before/after timings.
- **`#neighborhoodlimit0`** (subagent `tsift-nblimit`, `~/work/btakita/worktrees/tsift-nblimit`):
  - Make `--limit 0` lift the per-kind caps too.
  - Report the dropped counts per kind and in total in the JSON and envelope, as additive fields.
  - Re-check the hidden `ChatMessageV0` declarations against the real data.
- **`#sdktraceroot`** is gated: it needs `#graphrefreshperf`, because the root-graph import depends on a refresh that finishes.
- **`#jaicrate`** is still running.

Preflight warnings:
- The supervisor's stale-binary recycle is already scheduled for the next idle boundary, so nothing needs doing.
- **Operator action:** the JetBrains plugin is 0.2.484 and this build ships 0.2.485. Run `agent-doc plugin install jetbrains`, then reload the IDE.

The exchange is now 203 lines, but the queue is active, so I'm not compacting.

### Re: #jaicrate done — opus-5.5 · 2026-10-03T03:21-04:00

> **Queue prompt:** do [#jaicrate]

**Done.** The subagent pushed `bc55362` to `main` without force, rebased onto `c05c7af`. `make check` exit 0: **3837 passed, 0 failed**. It now includes an all-languages pass. Installed with `--features all-languages`, and a `.jai` fixture still indexes: `graph length --callers` returns `main`.
- **New crate `packages/tsift-tree-sitter-jai`:**
  - Version 0.1.102, `license = "MIT-0"`, with upstream's LICENSE and a provenance README (constantitus/tree-sitter-jai@96440b0).
  - `bindings/rust/{build.rs,lib.rs}` mirror upstream's binding and expose `LANGUAGE` and `NODE_TYPES`. Upstream ships no other query constants.
  - The `build.rs` compiles `scanner.c` too. Upstream's script omits it, so upstream's own crate as written would not link.
  - The C sources were moved with `git mv` and are byte-identical. `node-types.json` was added from upstream.
  - Package: 14 files, **1.2 MiB compressed**, under the 10 MB limit. `cargo package` verify passes.
- **`tsift-graph`:** depends on it as `tree-sitter-jai = { package = "tsift-tree-sitter-jai", …, optional = true }`, so switching to an upstream `tree-sitter-jai` crate is a one-line change. Its `build.rs`, `vendor/`, `cc` and `tree-sitter-language` dependencies are gone.
- **Opt-in:** `lang-jai` is out of `default` in the root, `tsift-graph` and `tsift-search`, and stays in `all-languages`. With default features, `cargo tree -i tsift-tree-sitter-jai` finds nothing, and neither the `tsift` nor the `tsift-graph` package contains `parser.c`.
- **CI:** `make check` now also runs clippy and `cargo test --workspace --features all-languages`. The Jai tests run there: 1,908 tests with all languages against 1,905 by default, the extra three being the Jai-specific named tests. Each workspace test pass takes about 2 minutes.
- **Release:** `release.yml` builds with `--features all-languages`. `tsift-tree-sitter-jai` is published and package-checked before `tsift-graph`. The version-consistency check covers it automatically.
- **Docs:** `specs/structural-patterns.md`, `specs/architecture.md`, the README (install notes for `--features lang-jai` and `all-languages`), and VERSIONS.

**Before the next release (`#jaicratepublish`, gated on the operator):** crates.io trusted publishing only works for crates that already exist. `tsift-tree-sitter-jai` needs one manual token publish, plus its trusted publisher registered on crates.io. Otherwise the next release's publish job fails at this crate, before `tsift-graph`.

**Risks:**
- The next release will be the first to compile the 29 MB `parser.c` on the Windows (MSVC) and macOS runners.
- `all-languages` also adds the grammars used only for structural matching, so release archives and build times grow.

`#graphrefreshperf` and `#neighborhoodlimit0` are still running in subagents. The exchange is 225 lines, but the queue is active, so I'm not compacting.
<!-- agent:boundary:d4c17bb5:tsift -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
- ~~do [#jaicrate]~~
- ~~do [#xsdcontract]~~
- ~~do [#wsdlcontract]~~
- ~~do [#releasepublishbindings]~~
- ~~#subagent: Add C bindings~~
- ~~#subagent: Add Odin bindings~~
- ~~#subagent: Add Jai bindings~~
- ~~#subagent: [#sdktsiftedges]~~
- ⏭️ do [#graphrefreshperf]
- do [#neighborhoodlimit0]
- ~~do [#sdktraceroot]~~
<!-- /agent:queue -->
<!--
https://github.com/chopratejas/headroom
-->

## Backlog

<!-- agent:backlog priority queue -->
- [/] [#jaicratepublish] [operator-verify] Before the next tsift release: publish tsift-tree-sitter-jai 0.1.102 to crates.io once by hand with a token (cargo publish -p tsift-tree-sitter-jai, token in passage), then register the btakita/tsift release.yml trusted publisher for the crate on crates.io. The release publish job uses OIDC trusted publishing, which only works for crates that already exist, so without this the next release fails at this crate before tsift-graph. Gated on operator approval of the publish and the crates.io settings change.
- [ ] [#graphrefreshperf] Root-scope graph-db refresh over the haiven-sdk tree (sdk-ruff worktree of the feat/codegen-trace branch) ran >13 min without finishing; ~50% of samples in link_ast_navigation_edges plus heavy libc. Profile, fix the hot path, add a perf-gate case.
- [ ] [#neighborhoodlimit0] graph-db neighborhood: the default output budget drops nodes per kind (11 ChatMessageV0 declarations hidden) and --limit 0 does not lift it. Make --limit 0 mean unlimited (or add --all), and report the dropped counts in the envelope.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#sdktraceroot] [recommended] After #graphrefreshperf: import haiven-sdk codegen/trace.json into the ROOT graph so per-language declarations join by path:line, and investigate unjoined trace nodes (8/51 contracts, 10/23 events, 56/195 REST ops, 50 components); check for a contracts-revision mismatch vs the trace pin.
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
