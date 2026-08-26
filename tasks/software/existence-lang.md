---
agent_doc_session: d076f48f-239e-4d31-95b9-ddaa58e0e1da
resume:
  claude: a043d832-73c7-4b49-8526-5651b7c5a860
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


### Re: Can existence-lang integrate with OWL, RDF, and SPARQL? — fable

> **Queue prompt:**
>
> do [#existencelangintegrate]


> **Queue prompt:** Can existence-lang integrate with ontology technologies such as OWL, RDF, and SPARQL?

**Short answer:** yes for RDF and SPARQL, with a clean and mostly lossless mapping; partially for OWL, where the honest fit is SKOS-on-OWL rather than a class hierarchy. Nothing exists today: `existence` v0.4.1 depends only on clap, serde, toml, and regex, and the only "owl" hits in the tree are substrings of "knowledge".

#### What existence-lang actually is, in RDF terms

The data model is small, which is why the mapping is easy:

| existence-lang | RDF / SKOS target | Notes |
|---|---|---|
| node file `src/{term}.md` | `skos:Concept` with IRI `{base}/{term}` | one node = one resource |
| `# Title` | `rdfs:label` / `skos:prefLabel` | |
| lay definition (first Ontology sentence, SPEC rule 2) | `skos:definition` | the spec already mandates a one-line definition, so this is free |
| Ontology / Axiology / Ethics / Epistemology sections | annotation properties in a tiny `xl:` vocabulary (`xl:ontology`, `xl:axiology`, ...) | prose literals; `lookup --json` already extracts exactly these four |
| `[term](./term.md)` links | `skos:related` | links are untyped, so this is the only relation the exporter can prove |
| `<a href="...">` cultural refs | `rdfs:seeAlso` / `dcterms:source` | |
| rings in `existence.toml` | `skos:Collection` per ring, `skos:member` | or `skos:ConceptScheme` + `skos:inScheme` for the whole ontology |
| domain overlay `Derives From: project + story` | `skos:broader` to each parent | SKOS allows multiple broader; OWL would need an intersection class |

`existence graph --format json` is already a serialized adjacency list of exactly this graph, and `lookup --json` is one `@context` away from being JSON-LD, which is RDF. That is the cheapest possible first step: no new dependencies, just an `@id` and a context document.

#### RDF: yes, lossless

Everything the CLI parses today round-trips into Turtle or JSON-LD without losing information. The whole ontology is 114 terms, so a hand-rolled Turtle writer is simpler than pulling in an RDF crate; validate the output with `riot --validate` (Jena) or `rapper` in CI. Publishing `ontology.ttl` on `existence-lang.github.io` makes every term a dereferenceable IRI.

The one thing that is lossy is edge typing. A `[term](./term.md)` link means "this concept mentions that concept", nothing more, so the exporter cannot distinguish broader/narrower/part-of from a passing reference. Getting `skos:broader` requires either a spec change (a link title attribute like `[term](./term.md "broader")`, or a frontmatter `relations:` block) or inferring from position (Ontology-section links are stronger than Epistemology-section links; `#### [Being](./being.md)` subheads under Pattern Expression read as narrower). That is a naming/spec discussion, not something to bolt on quietly.

#### OWL: yes as a vocabulary, no as a class hierarchy

The tempting mapping is term → `owl:Class`, link → `rdfs:subClassOf`. Do not do that. The ontology's own rules forbid it: "every term applies to every entity at every scope", "all entities are Systems, have Perspectives, expose Abstractions". Existence → Entity → System → Domain is a chain of scoping perspectives, not set inclusion, and a DL reasoner would either find nothing or find contradictions (Existence contains itself; God = Existence; a term that is both a member of Ring 0 and an annotation on every other term). The node template is deliberately prose in three modes (what it is, why it matters, how we know it), and OWL has no place for axiology or epistemology at all.

What OWL *is* good for here:
- A small **metamodel vocabulary** (`xl:Term`, `xl:ring`, `xl:derivesFrom`, the four section annotation properties) published as an OWL ontology. That describes the node template, not the philosophy, and it is exactly what SKOS does for itself (SKOS is an OWL Full ontology in which `skos:Concept` is an `owl:Class`).
- Interop: tools like Protégé, TopBraid, and PoolParty consume SKOS natively, so an "OWL integration" for most consumers just means "publish valid SKOS".

If someone later wants a genuinely axiomatized subset (say, the Ring 1 software terms as a DDD vocabulary with disjointness), that is a separate, hand-authored artifact, not an export.

#### SPARQL: free once RDF exists

Once there is a Turtle file, any triple store answers SPARQL over it. Two integration levels:
1. **External:** `riot`, `arq`, Oxigraph server, GraphDB, etc. load `ontology.ttl` directly. Zero code.
2. **Built in:** `existence sparql '<query>'` and a `/sparql` endpoint on the already-planned `serve` command, using the `oxigraph` crate (pure Rust, embeddable, supports SPARQL 1.1 query/update). Oxigraph is a heavy dependency, so it belongs behind a cargo feature.

Queries that would actually be useful and that the current CLI cannot answer: terms with no inbound links (an orphan lint), the transitive `derivesFrom` closure of an overlay term, "every Ring 0 term that links to `consciousness` from its Ontology section", and cross-ontology joins once a project publishes its overlay as its own scheme with `skos:inScheme`.

#### The reverse direction

Importing an external OWL/SKOS vocabulary into existence nodes is a scaffold, not a round-trip: `skos:prefLabel` and `skos:definition` map to the title and lay definition, but nothing external carries axiology or epistemology, so generated nodes fail `existence lint` until a human writes those sections. That is arguably correct behavior: the whole point of the template is that a definition without "why it matters" and "how we know" is incomplete.

#### Recommendation

Do it in this order, each a small change: (1) decide the base IRI, (2) ship `export --format turtle|jsonld` with the SKOS mapping and CI validation, (3) publish the `.ttl` on the site, (4) add SPARQL via oxigraph behind a feature once `serve` exists. Typed links are the only piece that touches the spec, and they should be discussed before any syntax lands. All four are filed as `[recommended]` backlog items below; none are queued, so nothing runs until you promote one.
<!-- agent:boundary:39531613 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue go priority -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#xlexportskos] [recommended] `existence export --format turtle|jsonld` ... SKOS mapping (term→skos:Concept, lay definition→skos:definition, `[term](./term.md)` links→skos:related, rings→skos:Collection, external `<a href>` refs→rdfs:seeAlso). Hand-rolled Turtle writer + a `@context` for `lookup --json`; validate output with `riot --validate` in CI. Blocked on the base-IRI decision in [#xlbaseiri]
- [ ] [#xlbaseiri] [recommended] Decide the base IRI for term identity before any RDF export ships. Proposal: `https://existence-lang.github.io/ontology/{term}` (the site repo already exists; publish `ontology.ttl` there). Naming decision ... discuss to convergence first
- [ ] [#xltypedlinks] [recommended] Typed relations for richer SKOS/OWL: today every `[term](./term.md)` link is untyped so export can only emit skos:related. Options: link title attribute (`[term](./term.md "broader")`), a frontmatter `relations:` block, or derive broader/narrower from section position (Ontology-section links vs `#### [Sub](./sub.md)` Pattern Expression subheads) plus overlay `Derives From`→skos:broader. Spec change ... naming discussion first, then `existence lint` rule
- [ ] [#xlsparql] [recommended] `existence sparql <query>` and a SPARQL endpoint on the planned `serve` command via the `oxigraph` crate behind a cargo feature (keeps the default binary small). Depends on [#xlexportskos]
<!-- /agent:backlog -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done -->
- 2026-08-26 [#existencelangintegrate] Can existence-lang integrate with ontology technologies such as OWL, RDF, and SPARQL?
<!-- /agent:done -->
