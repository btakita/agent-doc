---
agent_doc_session: d076f48f-239e-4d31-95b9-ddaa58e0e1da
resume:
  claude: a043d832-73c7-4b49-8526-5651b7c5a860
agent: claude
claude_model: fable
agent_doc_format: template
agent_doc_write: crdt
queue: stop
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
<!-- agent:boundary:39531613 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority go -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
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
<!-- /agent:done -->
