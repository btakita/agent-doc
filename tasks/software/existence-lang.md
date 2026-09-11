---
agent_doc_session: d076f48f-239e-4d31-95b9-ddaa58e0e1da
resume:
  claude: a043d832-73c7-4b49-8526-5651b7c5a860
agent: claude
claude_model: fable
agent_doc_format: template
agent_doc_write: crdt
queue: go
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/856921561b0186a44005bdcf3ba664046ec8d73bc5511ec83f3294de47388042-20260911-172902.md`*

Compacted content:
- Archived 9 response topic(s): Can existence-lang integrate with OWL, RDF, and SPARQL?; do #xlexportskos; do #xlbaseiri; 6 more

### Re: do #fixhttpsgithub — fable

> **Queue prompt:**
>
> do [#fixhttpsgithub]


Done. [existence-lang/existence#1](https://github.com/existence-lang/existence/issues/1) asked for an `existence toc` command: a term index with lay definitions, grouped by ring. It shipped in [`28478fb`](https://github.com/existence-lang/existence/commit/28478fbf039b1607baeb78518175160dfaefccb7) on `main`, the issue auto-closed, and I left a usage comment on it. CI run [34628503947](https://github.com/existence-lang/existence/actions/runs/34628503947) was still in progress at closeout; local `make check` is green (69 tests, clippy on both feature sets, fmt).

```
existence toc [--format markdown|json] [--ring N] [--base PATH] [-o FILE] [--check]
```

What it does, against the proposal in the issue:

- **Grouped by ring, manifest order.** `## Ring N — name` heading with the ring description from `existence.toml`, then one `- [**Title**](path) — lay definition` bullet per term. The lay definition is the first Ontology line (SPEC rule 2), the same extraction `search` uses.
- **Link rewriting (`--base`).** `[text](./term.md)`, with an optional `#anchor` or `"broader"` title, becomes `[text](<base>/term.md…)`. Without `--base`, `-o FILE` uses the relative path from the file's directory to `src/` (`-o TERMS.md` at the root gives `src/term.md`, `-o docs/terms.md` gives `../src/term.md`, `-o src/README.md` gives `./term.md`); stdout defaults to `src`.
- **Title vs slug.** Markdown shows the `# Title`; JSON carries `term` (slug), `title`, `path`, `definition` (markdown, rebased) and `definition_text` (plain) per entry, plus `missing` per ring and a top-level `unringed` list.
- **Manifest check.** Terms in `src/` that no ring declares land under `## Unringed`; ring terms with no node file are marked `*missing*` inline.
- **`--check`.** Emits nothing, fails on: an Ontology section that does not open with a plain sentence (blockquote, heading, list, table, fence, HTML tag, image, footnote), a ring term with no file, a node in no ring. Prints one `term: message` line per problem to stderr.

Implementation: new `src/commands/toc.rs` (build model → markdown / JSON renderers → `problems()`), `markdown::extract_title` made public, `Toc` variant in `main.rs`, README section and command-table row. 11 unit tests cover ring grouping and manifest drift, link rebasing with anchors and titles, empty base, ring filter, exact markdown and JSON shape, the check findings, the plain-sentence heuristic, the `-o` relative-base default at three depths, and the relative-path helper. The two `AGENTS.md`/`CLAUDE.md` edits already sitting in that worktree are another session's; I committed only the six toc files.

Run against the reference ontology, `--check` already finds real drift, which is the "doubles as a manifest check" point from the issue:

```
qualitative: declared in ring 1 of existence.toml but src/qualitative.md does not exist
quantitative: declared in ring 1 of existence.toml but src/quantitative.md does not exist
contextual-systems: Ontology section is missing or empty (no lay definition)
noumena: not declared in any ring of existence.toml
phenomena: not declared in any ring of existence.toml
pneuma: not declared in any ring of existence.toml
```

Two follow-ups are in backlog: a 0.6.0 release so installer users get the command (the local `cargo install --path .` binary has it, but still reports 0.5.0), and fixing that ontology drift plus wiring `toc --check` and a published `terms.md` into the ontology repo's CI/Pages workflow.
> **Queue prompt:**
>
> do [#fixhttpsgithub]
<!-- agent:boundary:39531613 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority go -->
- ~~do [#fixhttpsgithub]~~
- do [#planwayregularly]: The audit should also identify + report + safely resolve (where possible): contradictions + source/ontology drift.
- do [#xlgenusport]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority -->
- [ ] [#cutexistenceusers] [recommended] Cut existence 0.6.0 (`make release` in `src/existence-lang/existence`) so `install.sh` / `cargo install existence` users get `toc`; `existence --version` on `cargo install --path .` still reads 0.5.0. Bump `Cargo.toml` first, then flip the README "Commands (v0.3.0)" heading, which is already stale
- [ ] [#fixmanifestdrift] Fix the manifest drift `existence toc --check` found in `src/existence-lang/ontology`: `qualitative` + `quantitative` are declared in ring 1 with no node file (create them or drop them from the ring), `contextual-systems` has an empty Ontology section (needs a lay definition), `noumena` / `phenomena` / `pneuma` are in no ring (ring 2 fits), and `evolution.md` links `./environment` without `.md` so neither lint nor the rebaser sees it. Then add `existence toc --check` next to `lint` in the ontology CI and publish `terms.md` from the Pages workflow (`existence toc -o terms.md`) so the site has a glossary page
- [ ] [#planwayregularly] Plan a way to regularly audit ontology items. Should we track the sources of each ontology item and recheck during the audit?
- [ ] [#xlgenusport] [recommended] Port the two genus decisions from `#xlchaingaps` to the sibling `philosophy/` repo (what `~/.claude/philosophy` resolves to; its `src/entity.md` still reads "Any information in Existence" and `src/abstraction.md` still says "a concept that models") and to the `<!-- philosophy:start -->` table in `~/.claude/CLAUDE.md` (Entity row). Content, two files plus one table row; decide whether `philosophy/` should track `existence-lang/ontology` instead of drifting
<!-- /agent:backlog -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done -->
- 2026-08-26 [#existencelangintegrate] Can existence-lang integrate with ontology technologies such as OWL, RDF, and SPARQL?
- 2026-08-27 [#xlexportskos] [recommended] `existence export --format turtle|jsonld` ... SKOS mapping (term→skos:Concept, lay definition→skos:definition, `[term](./term.md)` links→skos:related, rings→skos:Collection, external `<a href>` refs→rdfs:seeAlso). Hand-rolled Turtle writer + a `@context` for `lookup --json`; validate output with `riot --validate` in CI. Blocked on the base-IRI decision in [#xlbaseiri]
- 2026-08-27 [#xlbaseiri] [recommended] Decide the base IRI for term identity before any RDF export ships. Proposal: `https://existence-lang.github.io/ontology/{term}` (the site repo already exists; publish `ontology.ttl` there). Naming decision ... discuss to convergence first
- 2026-08-27 [#xltypedlinks] [recommended] Typed relations for richer SKOS/OWL: today every `[term](./term.md)` link is untyped so export can only emit skos:related. Options: link title attribute (`[term](./term.md "broader")`), a frontmatter `relations:` block, or derive broader/narrower from section position (Ontology-section links vs `#### [Sub](./sub.md)` Pattern Expression subheads) plus overlay `Derives From`→skos:broader. Spec change ... naming discussion first, then `existence lint` rule
- 2026-08-27 [#xlsparql] [recommended] `existence sparql <query>` and a SPARQL endpoint on the planned `serve` command via the `oxigraph` crate behind a cargo feature (keeps the default binary small). Depends on [#xlexportskos]
- 2026-08-27 [#xltypekernel] [recommended] Type the Ring 0 kernel links: for each of the 14 kernel nodes in `src/existence-lang/ontology/src`, retitle the Ontology-section links that are genuinely hierarchical at the term's own scope (`[information](./information.md "broader")`, `[system](./system.md "narrower")`), leave passing mentions untitled, run `existence lint` (warns on conflicts) and `existence export` to check the resulting skos:broader/narrower graph reads correctly. This is editorial ... the philosophy decides what is broader, so review each edit
- 2026-08-27 [#xlpublishttl] Publish `ontology.ttl` + `ontology.jsonld` from the ontology Pages workflow: before the Jekyll build step in `src/existence-lang/ontology/.github/workflows/pages.yml`, install `existence` and run `existence --ontology . export` (+ `--format jsonld`) so `https://existence-lang.github.io/ontology/ontology.ttl` resolves. Needs the `export` command in an installable binary first ... push `existence@f354da3` and either cut a release (`make release`, bumps to 0.5.0) or `cargo install --git` in the workflow. Then verify live: `curl -sI https://existence-lang.github.io/ontology/entity` is 200 and `rapper -i turtle -c` parses the published .ttl
- 2026-08-28 [#xlrelease050] [recommended] Cut existence 0.5.0 (`make release` in `src/existence-lang/existence`) so `cargo install existence` / `install.sh` users get `export`, typed links, `sparql`, and `serve`; then switch `EXISTENCE_REV` in `ontology/.github/workflows/pages.yml` to the release tag. Today the Pages workflow pins commit `1fe3973` via `cargo install --git`, so the site does not depend on a release
- 2026-08-28 [#xlchaingaps] [recommended] Three edges of the canonical chain (Existence → Entity → System → Domain → Scope → Context → Resolution → Focus) cannot be typed by retitling because the Ontology section has no such link: `system.md` never links `entity`, `resolution.md` links `scope`/`perspective` but not `context`, `focus.md` never links `resolution`. Also two mutual definitions block typing in either direction: `entity.md` ("any information in Existence") vs `information.md` ("An Entity"), and `abstraction.md` ("a concept that models") vs `concept.md` ("an abstraction that does not represent a physical entity"). Decide which side is the genus and reword one sentence per pair; add the three missing chain sentences; then retitle. Content, not code
- 2026-09-11 [#fixhttpsgithub] Fix https://github.com/existence-lang/existence/issues/1
<!-- /agent:done -->
