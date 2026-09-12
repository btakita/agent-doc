---
agent_doc_session: d076f48f-239e-4d31-95b9-ddaa58e0e1da
resume:
  claude: a043d832-73c7-4b49-8526-5651b7c5a860
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
prompt_presets:
  '#actionable': 'Add actionable tasks into backlog + queue'
  '#actionable-review': 'Add actionable review items into backlog + queue'
  'rebase-conflicts': 'rebase + fix merge conflicts'
queue: go
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/856921561b0186a44005bdcf3ba664046ec8d73bc5511ec83f3294de47388042-20260912-034542.md`*

Compacted content:
- Archived 23 response topic(s): do #fixhttpsgithub; do #planwayregularly; do #xlgenusport; 20 more
- Prior summary/context: prior compacted content: Archived 9 response topic(s): Can existence-lang integrate with OWL, RDF, and SPARQL?; do #x...

### Re: do [#xlthreshrev] — opus-5

> **Queue prompt:**
>
> do [#xlthreshrev]


Shipped. existence 0.8.1 is tagged and the three workflows now pin it.

**existence** (`existence-lang/existence`, `ea3fa2b`, tag `v0.8.1`)
`Cargo.toml` 0.8.0 → 0.8.1 plus the lockfile; `make check` green (clippy `-D warnings`, 101 tests, then both again under `--features sparql` at 103, `cargo fmt --check`); `make release` tagged, pushed, and reinstalled `existence`/`xist` locally. `git tag --contains` confirms both rule commits — ddc0cc9 (near-duplicate needs 0.25 definition overlap on top of a shared stem) and 78df5f3 (table-mirror wording drift is opt-in) — are in `v0.8.1` and in no earlier tag, which is exactly why the weekly issue kept re-reporting them.

**ontology** (`existence-lang/ontology`, `38138f6`)
`EXISTENCE_REV: v0.8.0` → `v0.8.1` in `.github/workflows/{audit,ci,pages}.yml`. Pushed.

**Verification on the new binary**, from the ontology root:

```
$ existence --version
existence 0.8.1
$ existence --ontology . audit --structure --mirrors --format json
{ "findings": [], "summary": { "errors": 0, "warnings": 0, ... }, "clean": true }
```

Zero findings — the three near-duplicate pairs (abstract/abstraction, exist/existence, redefine/redefinition) and the nine `~/.claude/CLAUDE.md` kernel rows are all gone. Those were the whole standing false-positive set, so next Monday's audit issue should come back clean on those two checks.

**CI, recorded as pending, not waited on:** the Release run (`34671312780`) is in progress across all seven build targets; ontology CI (`34671335056`) and Pages (`34671335119`) are queued on the pin bump. As the backlog item anticipated, the tag also fires the crates.io publish job, which will 403 until [#cratestoken] is resolved — the GitHub Release and the tag land regardless, and `cargo install --git --tag` only needs the tag, which is what all three workflows use.

### Re: do [#xlmirrorcover] — opus-5

> **Queue prompt:**
>
> do [#xlmirrorcover]


Decided, implemented, released, and declared on the real table.

**The decision.** `rings = [0]` on a `[[mirrors]]` entry is a **completeness claim**: the table's rows are exactly the terms of those rings. The two questions the item asked:

- **Several rings** → their **union**. `rings = [0, 1]` wants a row for every term of either ring and accepts a row from either. Union is the only reading that composes, because ring term lists are disjoint — an intersection would always be empty.
- **A deliberate subset of one ring** → **not expressible, on purpose.** Such a table omits `rings` and keeps today's behaviour (each row must name a real node; nothing is required to be present). A claim that permits arbitrary omission detects nothing, which is exactly the hole this key exists to close. If a real table ever needs "ring 0 minus three terms", that is a separate `except` feature to add when something needs it — adding it now would let the coverage check be silently defeated, the same failure mode as [#xlthreshrev] and [#xlmentionkeep].

Two further rules fall out: `rings` on a `nodes` or `toc` mirror is a config error (`toc` mirrors are regenerated whole; `nodes` mirrors deliberately tolerate extras), and naming an undeclared level is a config error too — the same typo protection `compare` already has, since a typo would silently claim coverage of nothing.

**The code** (`existence-lang/existence` `9c9494e`, tag `v0.9.0`). `Mirror.rings: Vec<u32>`; validation in `check()`; `table_mirror` collects the union of declared rings and the set of row terms and diffs both ways. Two new check names so the report stays legible:

- `mirror_coverage` — a declared-ring term with no row (the omission the item is about)
- `mirror_extra_row` — a row naming a term outside the declared rings (the table grew past its claim)

Five regression tests, including `without_a_rings_claim_coverage_is_not_checked` — that one guards the claim itself, so a later refactor cannot make coverage unconditional and break every partial-copy mirror. `make check` green: 106 tests, 108 under `--features sparql` (from 101/103), clippy `-D warnings`, `fmt --check`.

Minor bump, not a patch: this adds user-visible config surface, and every prior feature in this repo took a minor. It matters practically too — `Mirror` does not `deny_unknown_fields`, so a v0.8.1 binary reads `rings = [0]` and silently ignores it. Shipping the key without the pin would have left the claim inert in CI, which is the same mistake [#xlthreshrev] had just finished cleaning up.

**Declared on the real table** (`existence-lang/ontology` `195e840`): `rings = [0]` on the `~/.claude/CLAUDE.md` mirror, and `EXISTENCE_REV` → `v0.9.0` in all three workflows.

**Verification.** A clean result proves nothing on its own, so I made the check fire first. Widening the live claim to `rings = [0, 1]` reported 17 `mirror_coverage` findings (`algorithm`, `coherence`, `collective`, …); restoring `rings = [0]` returned clean. So the installed binary is running the new code and the table is genuinely complete today — 14 rows, 14 Ring 0 terms, in the same order. The full audit now reports `errors: 0` with structure and mirrors clean; the only 11 warnings left are the `unlinked_mention` keeps, which is the next queue item.

**Release status.** The `v0.8.1` run finished with only the crates.io `publish` job failing — the [#cratestoken] 403, as expected. GitHub Release `v0.8.1` exists with 7 assets, which is all `cargo install --git --tag` needs, so the pin bump in the previous item was never at risk. The `v0.9.0` Release run is in progress and will fail the same publish job for the same reason; ontology CI and Pages are queued on the pin bump.
<!-- agent:boundary:39531613 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority go -->
- ~~do [#xlthreshrev]~~
- ~~do [#xlmirrorcover]~~
- do [#xlmentionkeep]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority -->
- [ ] [#xlmentionkeep] Give the audit a way to record a reviewed-and-intentionally-unlinked mention, so `contradictions/unlinked_mention` stops re-listing the 11 permanent keeps every weekly run: axiology nature/type, ontology type, domain unique, type science (ordinary English in a borrowed sentence), consciousness being, unique being (a participle, not the noun), belief-system system, evolutionary-linguistics linguistics (the node's own name), physical-system system (self-reference), epistemology concept (inside a quoted example), and exist being (left unlinked permanently because being.md is "To be, to exist", so the link would recreate a mutual-definition cycle — [#xlgenuscycles] settled the other four pairs and deliberately left this one). Design decision first, then code: the marker could be an `[audit]` allow table in `existence.toml` keyed by term+mention, an inline HTML comment in the node, or a SPEC rule that exempts quoted/borrowed sentences from the scan. Same class of problem as [#xlthreshrev] — a check whose true positives are already resolved but unrecordable trains the reader to skip the report. Discuss the shape before editing
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#cratestoken] Mint a new crates.io API token (https://crates.io/settings/tokens, scope publish-update, crate `existence`, long or no expiry): the one stored at `passage btak/CARGO_REGISTRY_TOKEN` and as the `CARGO_REGISTRY_TOKEN` secret on existence-lang/existence is a single token set 2026-08-28 that answered 403 `authentication failed` from 2026-09-12T00:06Z (it published v0.7.0 at 22:51Z the evening before), so existence 0.8.0 is on GitHub Releases and the tag but not on crates.io. Then `passage edit btak/CARGO_REGISTRY_TOKEN`, `passage show btak/CARGO_REGISTRY_TOKEN | gh secret set CARGO_REGISTRY_TOKEN -R existence-lang/existence`, and `gh run rerun 34660273106 --failed`; the publish job is idempotent
- [/] [#auditprtoken] Let the weekly Ontology audit workflow open its own `audit: safe fixes` pull request: the `existence-lang` organization forbids GitHub Actions from creating PRs, and lifting it needs an `admin:org` scope this session does not have (`gh auth refresh -h github.com -s admin:org`, then `gh api -X PUT orgs/existence-lang/actions/permissions/workflow -f default_workflow_permissions=read -F can_approve_pull_request_reviews=true` and the same PUT on `repos/existence-lang/ontology`), or add an `AUDIT_TOKEN` repo secret (classic PAT, `repo` scope) which `.github/workflows/audit.yml` already prefers. Until then the job pushes the branch and links the compare page in the issue; PR #2 was opened by hand and the job keeps it current
- [/] [#semantickey] Mint a Console API key (console.anthropic.com/settings/keys, `sk-ant-api03-...`) and store it with `passage edit btak/ANTHROPIC_API_KEY`: the entry today holds a Claude OAuth access token (`sk-ant-oat01-...`), which the Messages API rejects with HTTP 401, and `btak/OPENAI_API_KEY` is rejected too, so the semantic audit has only been proven against the local stand-in server. Then run `ANTHROPIC_API_KEY="$(passage show btak/ANTHROPIC_API_KEY)" existence --ontology src/existence-lang/ontology audit --semantic --limit 5` once to confirm the live Messages API shape, and `gh secret set ANTHROPIC_API_KEY -R existence-lang/ontology` to switch on the weekly semantic pass that `audit.yml` already gates on that secret (about 117 Opus 5 calls the first week, then only changed neighbourhoods)
<!-- /agent:review -->

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
- 2026-09-11 [#planwayregularly] Plan a way to regularly audit ontology items. Should we track the sources of each ontology item and recheck during the audit?
- 2026-09-11 [#xlgenusport] [recommended] Port the two genus decisions from `#xlchaingaps` to the sibling `philosophy/` repo (what `~/.claude/philosophy` resolves to; its `src/entity.md` still reads "Any information in Existence" and `src/abstraction.md` still says "a concept that models") and to the `<!-- philosophy:start -->` table in `~/.claude/CLAUDE.md` (Entity row). Content, two files plus one table row; decide whether `philosophy/` should track `existence-lang/ontology` instead of drifting
- 2026-09-11 [#reconcileotherlay] Reconcile the other 11 lay definitions that differ between `philosophy/src` and `src/existence-lang/ontology/src` (abstraction, concept, context, domain, information, noumena, phenomena, pneuma, scope, state, tool) — decide direction per term (ontology is canonical for the 110 shared nodes; philosophy keeps its 49 extras), port the sentence, and keep the CLAUDE.md table row in `philosophy/install.sh` in step. Until Phase 5 of `tasks/software/plan-ontology-audit.md` declares philosophy as a `kind = nodes` mirror, this list is the drift record
- 2026-09-11 [#xlaudit1] Phase 1 of `tasks/software/plan-ontology-audit.md`: `existence sources [term] --json` derives (url, citing term, quoted passages) from the anchor+blockquote markdown SPEC rules 7/8 already mandate, plus the `audit/sources.lock.json` schema and writer (no network yet). Done when a fixture node round-trips and the real ontology lists all 313 anchors
- 2026-09-11 [#xlaudit2] Phase 2 of `tasks/software/plan-ontology-audit.md`: `existence audit --structure` wraps lint + `toc --check` + links missing the `.md` suffix (`evolution.md` → `./environment`) + near-duplicate slugs (`signal`/`signals`, `human`/`humans`); JSON report with exit codes 0/1/2; `--fix` appends `.md` only. Then a CI job on ontology PRs
- 2026-09-11 [#xlaudit3] Phase 3 of `tasks/software/plan-ontology-audit.md`: `existence audit --contradictions`: typed-link cycles in the broader graph after inverses, 2-cycles in lay-definition links (the `entity`/`information` shape the canonical-chain work fixed by hand), unlinked mentions of known terms in lay definitions. Report only, both sentences side by side
- 2026-09-11 [#xlaudit4] Phase 4 of `tasks/software/plan-ontology-audit.md`: `existence audit --sources` re-fetches each lock URL (1 req/s, User-Agent, skip-not-fail on unreachable host), compares reachability / quote presence (fuzzy ≥0.9) / page hash, pins a Wayback snapshot; `--fix` swaps a dead link to the snapshot. Test against a local http fixture
- 2026-09-11 [#xlaudit5] Phase 5 of `tasks/software/plan-ontology-audit.md`: `[[mirrors]]` in existence.toml (`kind = nodes|toc`) + `existence audit --mirrors`; `--fix` regenerates toc mirrors (`terms.md`); register the `~/.claude/CLAUDE.md` philosophy table and the sibling `philosophy/` repo so the genus-port drift is reported per term. The nodes comparator must strip link titles (`"broader"`) before diffing: 5 of the 11 terms `#reconcileotherlay` was handed differed only by that, and it should compare the first Ontology paragraph, not the whole section, so a missing Axiology section is not reported as lay-definition drift
- 2026-09-11 [#xlaudit6] Phase 6 of `tasks/software/plan-ontology-audit.md`: weekly `schedule` workflow in `src/existence-lang/ontology` running `existence audit --all --format json`, upserting one `Ontology audit` issue (edit by title, diff vs last report) and one idempotent `audit: safe fixes` PR carrying only safe resolutions + refreshed lock + regenerated mirrors. Done after one real scheduled run
- 2026-09-11 [#xlaudit7] Phase 7 of `tasks/software/plan-ontology-audit.md`: `existence audit --semantic --provider anthropic|openai` LLM contradiction pass over each node's lay definition and its linked neighbours, report only, verdicts cached by content hash so a rerun with no changes makes zero model calls. Never on the PR path
- 2026-09-11 [#cutexistenceusers] [recommended] Cut existence 0.6.0 (`make release` in `src/existence-lang/existence`) so `install.sh` / `cargo install existence` users get `toc`; `existence --version` on `cargo install --path .` still reads 0.5.0. Bump `Cargo.toml` first, then flip the README "Commands (v0.3.0)" heading, which is already stale
- 2026-09-11 [#fixmanifestdrift] Fix the manifest drift `existence audit --structure` reports in `src/existence-lang/ontology` (it is what the new CI job fails on): `qualitative` + `quantitative` are declared in ring 1 with no node file (create them or drop them from the ring), `contextual-systems` has an empty Ontology section (needs a lay definition), `noumena` / `phenomena` / `pneuma` are in no ring (ring 2 fits), and `universal-set.md` links `./exists`, which has no node (`exist` does). The 11 fixable suffix-less links were already rewritten by `#xlaudit2`. Then publish `terms.md` from the Pages workflow (`existence toc -o terms.md`) so the site has a glossary page
- 2026-09-11 [#xlrequote] Decide what to do with the source drift the first real `existence audit --sources` run found in `src/existence-lang/ontology`: 206 quoted passages are no longer on their pages (87 wiktionary, 108 wikipedia, 6 etymonline; the nodes quote 2014-15 wording that has since been rewritten) and 19 have changed but still match. Per source, either refresh the blockquote from the live page or pin the historical Wayback snapshot next to the anchor so the quote stays verifiable. Also 6 dead links: `--fix` can swap cosmicscale.appspot.com (fractal, physical-system, scope), the NYT attention-schema piece, and wikipedia `Physical_information` (entity, information) for snapshots; stevenpoole.net, the AAAI SARA13 PDF, and the colorado.edu PDF have none and need a replacement source. Content decisions, not code; `audit --sources` reports them again each weekly run once Phase 6 commits the lock
- 2026-09-11 [#xlrequote2] Decide the 45 quotes no single Wayback snapshot carries (the strict pin rule wants every passage a node quotes from one page in one copy): 27 on Wiktionary (abstraction, conscious, context, create, creativity, evolution ×2, existence, explicit, focus, fractal, history, human ×2, intelligence, love, model, physical, science, spirit, story, system ×3, tool, type, violence) and 18 on Wikipedia (Abstract_object ×3, Algorithm, Emotion, Evolution_(disambiguation), Formal_system, Information ×2, Intelligence, Natural_law, Nature, Platonic_realism, Sexual_reproduction, Spirituality, Story, Technology, Type–token_distinction). Per source: split the blockquote so each passage sits under its own anchor and let `existence audit --sources --fix` pin each, or requote from today’s page. `audit --sources` lists them as `quote_missing` with no fix each week; see `tasks/software/existence-lang.md` for the pinning decision
- 2026-09-11 [#xldupenodes] Merge the three node pairs whose lay definitions are byte-identical (audit `structure/near_duplicate`, similarity 1.00): `agree`/`agreement`, `human`/`humans`, `signal`/`signals` in `src/existence-lang/ontology/src`. Per pair keep one slug, redirect inbound `[x](./x.md)` links, drop the other from its ring in `existence.toml`, rerun `existence audit --structure`. The other three pairs it flags are genuinely distinct (`abstract`/`abstraction` 0.03, `exist`/`existence` 0.00, `redefine`/`redefinition` 0.00) ... keep them and consider an allowlist so the check stops reporting them
- 2026-09-11 [#xlunlinked] Link the 32 lay definitions that name another node without linking it (audit `contradictions/unlinked_mention`), e.g. `aesthetics` mentions philosophy, `agree` mentions definition, `axiology` mentions nature and type. Each link densifies the skos:related graph and several are candidates for a typed `"broader"`. Editorial per mention ... a definition that uses "state" or "model" as ordinary English should stay unlinked, so review each rather than scripting it. `existence audit --contradictions` lists them with the exact term
- 2026-09-11 [#xlepistemsubs] Fix the 11 non-standard Epistemology subsections across 6 nodes (audit `structure/lint`): `intelligence` uses `Links` and `Human Intelligence`, `evolutionary-linguistics` uses a raw `<a href>` anchor as a heading, plus `perspective`, `presence`, `this-philosophy`, `unconsciousness`. Rename to the recognized vocabulary (`Cultural Definition` | `Sources`, `Pattern Expression` | `Examples`) or move the content under one of them; SPEC rule 9 keeps these advisory, so the alternative is widening the vocabulary in the spec
- 2026-09-11 [#xlgenuscycles] Decide the genus for the 4 remaining definitional cycles (audit `contradictions/mutual_definition`): aesthetics -> axiology, axiology -> ethics, coercion -> violence, qualitative -> quantitative. Same procedure `#xlchaingaps` used for entity/information: pick which side is the genus and reword one sentence so the pair stops defining each other. Content decision, four sentences
- 2026-09-11 [#xlclaudetable] Decide what the `~/.claude/CLAUDE.md` philosophy table is (audit `mirrors/mirror_drift`, 9 rows: context, domain, evolution, existence, focus, perspective, resolution, story, system). Every row summarises rather than quotes its node, so word overlap runs 0.00-0.21 and the mirror check calls it drift. Either reword the rows to the node lay definitions, making the table a real mirror, or declare the summary style intended and give the comparator a looser rule for `kind = toc` style mirrors. That file is your global instruction surface, so the direction is yours
- 2026-09-11 [#xlthreshrev] Ship the two audit-rule changes to the ontology workflows: `existence` ddc0cc9 gates `near_duplicate` on 0.25 definition overlap and 78df5f3 makes table wording-drift opt-in, but `.github/workflows/{audit,ci,pages}.yml` in `src/existence-lang/ontology` all pin `EXISTENCE_REV: v0.8.0`, so the weekly audit issue keeps reporting abstract/abstraction, exist/existence, redefine/redefinition and the nine `~/.claude/CLAUDE.md` kernel rows. Cut existence 0.8.1 (`make release`) and bump the three `EXISTENCE_REV` values to `v0.8.1`. Note the tag also runs the crates.io publish job, which 403s until [#cratestoken] is resolved; the GitHub Release and tag land either way, which is all `cargo install --git --tag` needs
- 2026-09-11 [#xlmirrorcover] Let a `table` mirror declare which rings it claims, so `compare = "paraphrase"` can check coverage and not only that each row maps to a node: `~/.claude/CLAUDE.md` is exactly the 14 Ring 0 kernel terms, so a new kernel term that never gets a row is silent drift today. Proposal: `rings = [0]` on the `[[mirrors]]` entry — warn on a declared-ring term with no row and on a row outside the declared rings (the existing no-node check already covers a row naming nothing). Design decision first: what a table mirroring several rings, or a deliberate subset of one, should mean
<!-- /agent:done -->
