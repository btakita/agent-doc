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

### Re: do #planwayregularly — fable

> **Queue prompt:**
>
> do [#planwayregularly]: The audit should also identify + report + safely resolve (where possible): contradictions + source/ontology drift.

Plan written to `tasks/software/plan-ontology-audit.md` (committed as `11416fd546`) and split into seven phase items at the top of the backlog. Short version below.

**Should we track sources per item? Yes, derived, not hand-kept.** Every node already records its sources in the shape SPEC rules 7 and 8 require: an `<a href>` anchor followed by `>` blockquotes of the passage relied on. Across the 117 nodes that is 313 anchors (119 Wiktionary, 144 Wikipedia, a few etymonline / YouTube / other) and 989 quoted lines. So the source record exists; what is missing is the part markdown cannot carry: when each URL was last fetched, what the page hashed to, whether the quoted passage was still there. That goes in a generated `audit/sources.lock.json` the audit refreshes and commits, keyed by URL with `cited_by`, `fetched_at`, `status`, `content_sha256`, per-term quote status (`present | moved | missing`), and a pinned Wayback snapshot. Recheck = re-fetch and compare against the lock. A dead link or a vanished quote is source drift; a changed page with the quote still present is noise and only recorded.

**Three check classes, each with an explicit safe-resolve boundary:**

- **Structure and manifest.** `lint` + `toc --check` (which already finds six real drifts today), plus links missing `.md` (`evolution.md` → `./environment`, invisible to both lint and the rebaser) and near-duplicate slugs (`signal`/`signals`, `human`/`humans`, `agree`/`agreement`, `redefine`/`redefinition`). Safe fix: append `.md` where the node exists. Ring membership stays a decision.
- **Contradictions.** Typed-link cycles in the broader graph after inverses; 2-cycles in lay-definition links (the `entity`/`information` and `abstraction`/`concept` shape that the canonical-chain work had to untangle by hand); unlinked mentions of known terms in a lay definition. All report-only with both sentences side by side. A semantic pass (LLM judges each node against the definitions it links, cached by content hash, behind a provider flag) is the last phase and never runs on the PR path.
- **Drift.** Source drift as above; safe fix is swapping a dead link for the pinned snapshot. Mirror drift: surfaces that copy definitions out of `ontology/src` and rot, declared as `[[mirrors]]` in `existence.toml` with `kind = "nodes"` (the `philosophy/` repo, the `~/.claude/CLAUDE.md` table; reported as a per-term diff, exactly the pending genus-port case) or `kind = "toc"` (`terms.md`; regenerated by `--fix`).

**Cadence.** The structural subset runs on every ontology PR and fails the check. Weekly, a scheduled workflow runs `existence audit --all --format json`, upserts a single `Ontology audit` issue (edit by title, diff against last week's report) and keeps one idempotent `audit: safe fixes` PR with only mechanical resolutions plus the refreshed lock and regenerated mirrors. Report JSON is the interface; exit 0 clean, 1 findings, 2 could not run. Fetches are rate-limited and an unreachable host skips the source pass rather than failing it.

**Phases**, one backlog item each (Phase 1 through 7): sources extraction + lock schema → `audit --structure` + PR CI → `--contradictions` → `--sources` with Wayback pinning → `[[mirrors]]` → weekly workflow with issue/PR upsert → `--semantic`. Phases 1 to 3 are offline and land the report format; 4 needs the lock from 1; 5 and 6 need 2; 7 last. Each phase closes with tests, `make check`, `cargo install --path .`, and a README section.

### Re: do #xlgenusport — fable

> **Queue prompt:**
>
> do [#xlgenusport]


Done. Pushed to `btakita/philosophy` as `a6a1497`, and the live `~/.claude/CLAUDE.md` table is updated.

**What was actually drifted.** Of the two genus decisions from the canonical-chain work, only one had a stale copy in `philosophy/src`:

- `src/entity.md` still opened with "Any [information] in [Existence]". Now "Anything in [Existence] that can be distinguished from anything else.", matching `existence-lang/ontology@0d2e495`. Untyped link, since the philosophy repo does not use the `"broader"` title convention.
- `src/abstraction.md` already names entity as the thing modeled ("a symbol that represents a real entity; member of a schema"), so the abstraction/concept loop was never present there. The stale "a concept modeling something else" wording lived in `philosophy/install.sh`, the script that generates the `<!-- philosophy:start -->` block in `~/.claude/CLAUDE.md`.

**The table has three copies, and the generator was the stale one.** The live CLAUDE.md section had moved ahead of `install.sh` (newer Abstraction row, plus a "Universal applicability" paragraph the generator lacked), so re-running the installer would have regressed it. I changed the Entity row in both, then synced the generator's kernel block to the live section byte for byte; `diff` of the two blocks is now empty and `bash -n` passes. `~/.claude` is not a git repo, so the generator is the versioned source for that table.

**Should `philosophy/` track `existence-lang/ontology`? Not as a submodule, yes as a declared mirror.** The numbers argue against a mechanical merge right now:

- 110 nodes are shared; 49 exist only in `philosophy/` (language-planning and the like); 7 only in the ontology (`abstract-system`, `agreement`, `conceptual-existence`, `contextual-systems`, `humans`, `physical-existence`, `signals`).
- Of the shared 110, 66 are byte-identical and 44 differ, mostly sections (the ontology added Axiology sections; philosophy lacks them on `entity` and `abstraction`).
- 12 shared nodes had a different lay definition before this cycle; entity is fixed, 11 remain: abstraction, concept, context, domain, information, noumena, phenomena, pneuma, scope, state, tool.

Direction: `existence-lang/ontology` is canonical for the shared nodes (linted, ring-declared, published); `philosophy/` is the personal superset that pulls decisions from it. Phase 5 of the audit plan declares `philosophy/src` as a `kind = "nodes"` mirror so the per-term lay-definition diff is reported weekly instead of rediscovered by hand, and `install.sh` should eventually generate its table from `existence toc --ring 0` rather than a hand-kept heredoc. A backlog item carries the 11-term reconcile, which is content work (each is a decision about which side is right, not a copy).

The philosophy worktree also holds another session's uncommitted `.gitignore` / `AGENTS.md` edits; I committed only `install.sh` and `src/entity.md`.

### Re: do #reconcileotherlay — fable

> **Queue prompt:**
>
> do [#reconcileotherlay]

Done. Pushed `btakita/philosophy@09e1a66` and `existence-lang/ontology@29566ed`; the live `~/.claude/CLAUDE.md` Abstraction row is updated and the `install.sh` kernel block still diffs empty against it. The 11 lay definitions now match on both sides, with one deliberate residue noted below.

**Five of the eleven were not drift.** `concept`, `context`, `domain`, `information`, `scope` differ only by the `"broader"` link title the ontology's typed-link pass added; the words are identical. Nothing to port, but the Phase 5 mirror comparator must strip link titles before diffing or it will keep reporting these (backlog text for `#xlaudit5` updated).

**Six needed a decision, and the direction went both ways:**

- **abstraction → ontology wins the genus, philosophy wins the enumeration.** The ontology's `0d2e495` sentence ("a symbol that *models* something else, allowing expression of the modeled entity without its complexity & ambiguity") is the two-week-old canonical-chain decision; philosophy's "represents a real entity" was five months older. But philosophy's follow-on ("a member of a schema; a word, an icon, an utterance, or any other representation") is better than the ontology's "a word or phrase", so the merged first paragraph now reads the same in both repos. The CLAUDE.md row became "A symbol that models an entity, expressing it without its complexity and ambiguity; member of a schema; often a simplification but not always".
- **noumena, phenomena, pneuma → philosophy wins.** The ontology versions (2026-03-31) opened without a genus ("The thing-in-itself…", "The observable manifestations of existence…", "The animating force or breath…") and leaned into agent-collaboration framing. The philosophy rewrites are eleven days newer and define each by genus in the entity chain: noumena are *entities as they are in themselves*, phenomena are *entities as they appear to some perspective*, pneuma is *the animating energy of an entity*. I ported those first paragraphs into the ontology and typed the genus link (`entity`, `entity`, `energy`) as `"broader"`, so `export` now emits `skos:broader` for all three. Bodies below the first paragraph are untouched on both sides.
- **state → ontology wins, and the fix was structural.** Philosophy's sentence was identical but filed under `## Axiology` with no Ontology section at all, which is why the extractor returned nothing. It now has the ontology's Ontology paragraph, and the misfiled sentence became the ontology's Axiology paragraph.
- **tool → identical sentence, missing heading.** Philosophy's definition sat above any section; it gained the `## Ontology` heading.

**One residue.** The ontology has no `life` node, so its `pneuma` sentence carries "life" as plain text where philosophy links `[life](./life.md)`. That is the 49-vs-7 node asymmetry from `#xlgenusport`, not drift.

**Verification.** Ontology `existence lint` is unchanged at 11 warnings, 0 errors; `toc --check` still reports exactly the six pre-existing manifest problems queued under `#fixmanifestdrift`; `export --format turtle` parses and shows the new broader edges. Philosophy has no `existence.toml`, so `existence --ontology . lint` reports 73 baseline errors across 58 files (missing Axiology sections, broken body links in the noumena/phenomena/pneuma bodies philosophy already had); none are in the lines I changed. `bash -n install.sh` passes. Pages run [34634612675](https://github.com/existence-lang/ontology/actions/runs/34634612675) was in progress at closeout; the previous run was green. Both worktrees still hold another session's `AGENTS.md`/`CLAUDE.md`/`.gitignore` edits; I committed only the four intended files in each.

Not fixed, worth knowing: the CLAUDE.md table's Scope row says "reach of a *perspective*" while both node files say "of an *Entity*; a domain". That is table-vs-node drift, which is the `kind = toc` mirror case for Phase 5 rather than this item.

### Re: do #xlaudit1 — fable

> **Queue prompt:**
>
> do [#xlaudit1]

Done. `existence sources` shipped in [`05766af`](https://github.com/existence-lang/existence/commit/05766af) on `main`; CI run [34635458324](https://github.com/existence-lang/existence/actions/runs/34635458324) was queued at closeout. Local `make check` is green (76 tests, up from 69; clippy on both feature sets; fmt) and `cargo install --path .` is done, so the local binary has it.

```
existence sources [term] [--json]          # list anchors + quoted passages
existence sources --lock [PATH]            # write audit/sources.lock.json
```

**Against the real ontology it lists all 313 anchors**, in all 117 terms, resolving to 282 unique URLs and 844 quoted lines. The 989 figure in the plan was a raw `grep '^>'` count; the difference is the `> ### Noun` headings and empty `>` spacer lines, which are not passages. 56 anchors carry no quotes at all: keynote videos, paper links, and mid-paragraph references like the Cosmic Scale link in `scope.md`, which the block rules treat correctly as a source with nothing quoted under it.

**Two source shapes, one extractor.** The nodes use both the standalone form (anchor line, then `>` lines; indented `>     sub-sense` lines and blank lines stay in the block) and the inline form `> <a href>Label</a>: passage`. A block ends at the next anchor, a heading, or any prose line. Labels wrapped onto the next line (the Cosmic Scale anchor) are recovered.

**Lockfile.** `--lock` aggregates by URL into exactly the plan's schema: `cited_by`, `fetched_at`, `status`, `content_sha256`, `quotes` (per citing term), `archive`. With no network this phase, new entries are `fetched_at: null` and `quotes: { term: "unchecked" }`. Rewriting an existing lock keeps the fetch results for URLs still cited and the quote status for terms still citing, and drops URLs and terms nothing cites any more, so Phase 4 can refresh in place. `--lock` with a term argument is rejected, since the lock always covers the whole ontology. I did not commit a lock into the ontology repo; the plan has the audit PR own that file once Phase 4 can populate it.

**Tests** (7, in `src/commands/sources.rs`): both shapes plus block termination and wrapped labels on one fixture node; an orphan blockquote before any anchor is ignored; `build` ordering, term filter, and unknown-term error; aggregation by URL with the exact pretty-printed lock JSON round-tripping through `write_lock`/`read_lock`; merge with an existing lock preserving fetch fields and dropping stale entries; the `--lock`/term conflict and the relative-path default; the exact text and JSON output shapes.

One content fix fell out of running it: `create.md` had the only anchor of the 313 with no label text (`<a href=…></a>`). Labelled it "Create (Wikipedia)" in `existence-lang/ontology@116fc11`. The `existence` worktree still holds another session's `AGENTS.md`/`CLAUDE.md` edits; I committed only the four files of this feature.

### Re: do #xlaudit2 — fable

> **Queue prompt:**
>
> do [#xlaudit2]

Done. `existence audit --structure` shipped in [`9414a8a`](https://github.com/existence-lang/existence/commit/9414a8a) (CI run [34636238853](https://github.com/existence-lang/existence/actions/runs/34636238853) in progress at closeout; the Phase 1 run before it is green). The ontology got its first PR/push CI job in [`2c32e36`](https://github.com/existence-lang/ontology/commit/2c32e36), together with the safe fixes the audit found. Local `make check` is green (81 tests, five new), and the local binary is installed.

```
existence audit [--structure] [--format text|json] [--fix] [-o FILE]
```

**What it wraps and what it adds.** Lint now exposes its findings as data (`lint::collect`), so the audit folds lint errors and warnings, the `toc --check` manifest problems, and two new checks into one report: node links that miss the `.md` suffix, which lint's link regex never counted and the toc rebaser never rewrote; and near-duplicate slugs, paired when two slugs share a candidate stem after one suffix strip, with the Jaccard similarity of their lay definitions attached so each pair reads as merge or keep. Every finding carries `class`, `check`, `severity`, `term`, `message`, `fix`, `fixed`. Exit 0 clean, 1 on any error-severity finding, 2 when the audit cannot run; warnings never fail it, which is what keeps the 11 known lint subsection warnings from turning CI permanently red.

**On the real ontology** it reported 18 errors and 17 warnings. The errors were the 6 manifest drifts from `#fixmanifestdrift` plus **12** suffix-less links, not the one the plan named: `./environment` in seven nodes, and `./being`, `./scope`, `./abstraction`, `./consciousness`, `./exists`. Eleven had an existing target and `--fix` rewrote them (the diff is `.md` insertions only, anchors and titles preserved); `universal-set.md` links `./exists`, which has no node, so it stays reported. The warnings are the lint vocabulary notes plus six near-duplicate pairs: the plan's four (`signal`/`signals` 1.00, `human`/`humans` 1.00, `agree`/`agreement` 1.00, `redefine`/`redefinition` 0.00) and two more the stemmer surfaced, `abstract`/`abstraction` (0.03) and `exist`/`existence` (0.00). The three at 1.00 have byte-identical definitions and look like merge candidates; the 0.00 pairs are verb/noun splits that are probably deliberate.

**CI.** `.github/workflows/ci.yml` in the ontology runs `audit --structure` on every pull request and push to `main`, pins `existence` by commit (cached like the Pages job; switch to a tag once `#cutexistenceusers` releases), prints the text view, and uploads the JSON report as an artifact. Run [34636271438](https://github.com/existence-lang/ontology/actions/runs/34636271438) will fail on the seven remaining errors until `#fixmanifestdrift` lands; that item's text is updated to what is actually left (the `evolution.md` link is done, the `./exists` link is added).

**Tests** (5 new): every check on one fixture ontology; `--fix` rewriting only existing targets and the second-pass residue; suffix rewriting with anchors and titles; the stem pairing with five negatives (`art`/`arts`, `spirit`/`spirituality`, `belief`/`believe`); and the clean report's exact JSON and text plus the class-defaulting and format-error paths. Both worktrees still hold another session's `AGENTS.md`/`CLAUDE.md` edits, which I left out of both commits. Exchange is large (5 responses); the queue is active so I did not compact.
<!-- agent:boundary:39531613 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority go -->
- ~~do [#reconcileotherlay]~~
- ~~do [#xlaudit1]~~
- ~~do [#xlaudit2]~~
- do [#xlaudit3]
- do [#xlaudit4]
- do [#xlaudit5]
- do [#xlaudit6]
- do [#xlaudit7]
- do [#cutexistenceusers]
- do [#fixmanifestdrift]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority -->
- [ ] [#xlaudit3] Phase 3 of `tasks/software/plan-ontology-audit.md`: `existence audit --contradictions`: typed-link cycles in the broader graph after inverses, 2-cycles in lay-definition links (the `entity`/`information` shape the canonical-chain work fixed by hand), unlinked mentions of known terms in lay definitions. Report only, both sentences side by side
- [ ] [#xlaudit4] Phase 4 of `tasks/software/plan-ontology-audit.md`: `existence audit --sources` re-fetches each lock URL (1 req/s, User-Agent, skip-not-fail on unreachable host), compares reachability / quote presence (fuzzy ≥0.9) / page hash, pins a Wayback snapshot; `--fix` swaps a dead link to the snapshot. Test against a local http fixture
- [ ] [#xlaudit5] Phase 5 of `tasks/software/plan-ontology-audit.md`: `[[mirrors]]` in existence.toml (`kind = nodes|toc`) + `existence audit --mirrors`; `--fix` regenerates toc mirrors (`terms.md`); register the `~/.claude/CLAUDE.md` philosophy table and the sibling `philosophy/` repo so the genus-port drift is reported per term. The nodes comparator must strip link titles (`"broader"`) before diffing: 5 of the 11 terms `#reconcileotherlay` was handed differed only by that, and it should compare the first Ontology paragraph, not the whole section, so a missing Axiology section is not reported as lay-definition drift
- [ ] [#xlaudit6] Phase 6 of `tasks/software/plan-ontology-audit.md`: weekly `schedule` workflow in `src/existence-lang/ontology` running `existence audit --all --format json`, upserting one `Ontology audit` issue (edit by title, diff vs last report) and one idempotent `audit: safe fixes` PR carrying only safe resolutions + refreshed lock + regenerated mirrors. Done after one real scheduled run
- [ ] [#xlaudit7] Phase 7 of `tasks/software/plan-ontology-audit.md`: `existence audit --semantic --provider anthropic|openai` LLM contradiction pass over each node's lay definition and its linked neighbours, report only, verdicts cached by content hash so a rerun with no changes makes zero model calls. Never on the PR path
- [ ] [#cutexistenceusers] [recommended] Cut existence 0.6.0 (`make release` in `src/existence-lang/existence`) so `install.sh` / `cargo install existence` users get `toc`; `existence --version` on `cargo install --path .` still reads 0.5.0. Bump `Cargo.toml` first, then flip the README "Commands (v0.3.0)" heading, which is already stale
- [ ] [#fixmanifestdrift] Fix the manifest drift `existence audit --structure` reports in `src/existence-lang/ontology` (it is what the new CI job fails on): `qualitative` + `quantitative` are declared in ring 1 with no node file (create them or drop them from the ring), `contextual-systems` has an empty Ontology section (needs a lay definition), `noumena` / `phenomena` / `pneuma` are in no ring (ring 2 fits), and `universal-set.md` links `./exists`, which has no node (`exist` does). The 11 fixable suffix-less links were already rewritten by `#xlaudit2`. Then publish `terms.md` from the Pages workflow (`existence toc -o terms.md`) so the site has a glossary page
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
- 2026-09-11 [#planwayregularly] Plan a way to regularly audit ontology items. Should we track the sources of each ontology item and recheck during the audit?
- 2026-09-11 [#xlgenusport] [recommended] Port the two genus decisions from `#xlchaingaps` to the sibling `philosophy/` repo (what `~/.claude/philosophy` resolves to; its `src/entity.md` still reads "Any information in Existence" and `src/abstraction.md` still says "a concept that models") and to the `<!-- philosophy:start -->` table in `~/.claude/CLAUDE.md` (Entity row). Content, two files plus one table row; decide whether `philosophy/` should track `existence-lang/ontology` instead of drifting
- 2026-09-11 [#reconcileotherlay] Reconcile the other 11 lay definitions that differ between `philosophy/src` and `src/existence-lang/ontology/src` (abstraction, concept, context, domain, information, noumena, phenomena, pneuma, scope, state, tool) — decide direction per term (ontology is canonical for the 110 shared nodes; philosophy keeps its 49 extras), port the sentence, and keep the CLAUDE.md table row in `philosophy/install.sh` in step. Until Phase 5 of `tasks/software/plan-ontology-audit.md` declares philosophy as a `kind = nodes` mirror, this list is the drift record
- 2026-09-11 [#xlaudit1] Phase 1 of `tasks/software/plan-ontology-audit.md`: `existence sources [term] --json` derives (url, citing term, quoted passages) from the anchor+blockquote markdown SPEC rules 7/8 already mandate, plus the `audit/sources.lock.json` schema and writer (no network yet). Done when a fixture node round-trips and the real ontology lists all 313 anchors
- 2026-09-11 [#xlaudit2] Phase 2 of `tasks/software/plan-ontology-audit.md`: `existence audit --structure` wraps lint + `toc --check` + links missing the `.md` suffix (`evolution.md` → `./environment`) + near-duplicate slugs (`signal`/`signals`, `human`/`humans`); JSON report with exit codes 0/1/2; `--fix` appends `.md` only. Then a CI job on ontology PRs
<!-- /agent:done -->
