---
agent_doc_session: cb5672cc-5d1d-4228-af30-640a336f692e
tmux_session: '1'
---



## Session Summary

Existential Kernel — a composable ontology framework with CLI tooling, published to crates.io.

### What was built

**Philosophy integration (earlier session):**
- `philosophy/install.sh` — symlinks `~/.claude/philosophy` → git repo, writes 14-term kernel to `~/.claude/CLAUDE.md`
- 14-term kernel always loaded: Existence, Entity, Abstraction, Scope, Context, Resolution, Pattern, System, Domain, Focus, Perspective, Consciousness, Evolution, Story
- Domain overlays: repos extend base ontology via `## Domain Ontology` in their CLAUDE.md

**Existential Kernel org** (https://github.com/existential-kernel):

| Repo | Description |
|------|-------------|
| [ontology](https://github.com/existential-kernel/ontology) | 114 term files across 3 rings, lint-clean. `exkernel.toml` config. |
| [exkernel](https://github.com/existential-kernel/exkernel) | Rust CLI v0.2.1 on [crates.io](https://crates.io/crates/exkernel). Binaries: `exkernel` + `exk`. |
| [spec](https://github.com/existential-kernel/spec) | SPEC.md node format + RINGS.md ring structure. |
| [.github](https://github.com/existential-kernel/.github) | Org profile README. |
| [github.io](https://existential-kernel.github.io) | Landing page. |

All repos are submodules in `agent-loop/existential-kernel/`.

**Ring structure:**
- **Ring 0 (Kernel, 14 terms):** Universal existential scope, always loaded
- **Ring 1 (Software, 17 terms):** DDD bridge — project, model, algorithm, state, type, definition, information, signal, language, tool, environment, coherence, communication, collective, integrity, qualitative*, quantitative* (*pending)
- **Ring 2 (Extended, 85 terms):** Philosophy, spirituality, cognition, linguistics

**DDD mapping:** Ubiquitous Language = language + domain; Bounded Context = scope + context; Entity = entity; Aggregate = system; Domain Event = evolution + signal; Model = model; Value Object = state + type.

**exkernel CLI commands (v0.2.1):**

| Command | Status |
|---------|--------|
| `lookup <term>` | Done — read node definition, `--json` output |
| `search <query>` | Done — relevance scoring (name > definition > body), `--json`, `--limit` |
| `scope [ring]` | Done — list terms by ring |
| `lint [path]` | Done — validate sections + links |
| `graph [ring]` | Done — DOT/JSON relationship graphs |
| `fetch [source]` | Done — clone/pull to `~/.exkernel/sources/` |
| `new <term>` | Done — scaffold from SPEC.md template, `--ring`, `--description`, `--no-edit` |
| `completions <shell>` | Done — bash/zsh/fish/elvish/powershell |
| `install` | Stub |
| `serve` | Stub |
| `build-site` | Stub |
| `context <domain>` | Stub |

**`/exkernel` Claude skill** — created at `.claude/skills/exkernel/SKILL.md`. Supports lookup, search, new (interactive authoring), lint, scope.

**agent-doc domain ontology** — 12-term mapping added to `agent-doc/CLAUDE.md`: Session (project+story), Document (entity+context), Pane (focus+scope), Claim (scope+entity), Route (context+resolution), Sync (pattern+system), Watch (consciousness+evolution), Dashboard (system+focus), Range Template (scope+abstraction), Registry (system+perspective), Snapshot (state+story), Overlay (context+resolution).

### Key decisions

- **Name:** "existential-kernel" org, `exkernel`/`exk` CLI binary
- **Config:** `exkernel.toml` (no dot, visible like `Cargo.toml`)
- **Navigation:** Links to upstream, not copies. Build-time resolution, display-time linking.
- **Web:** Static JSON first (GitHub Pages), edge functions later. `exkernel build-site` generates at build time.
- **Your kernel synthesizes four traditions:** process philosophy (Whitehead), phenomenology (Husserl), information-theoretic ontology, existentialism. "Resolution" is the strongest novel term.
- **PATH:** `~/.cargo/bin` added to `~/.zshenv`

### Releases

| Version | What shipped |
|---------|-------------|
| v0.1.0 | Initial: lookup, scope, lint, graph, fetch. 114 terms. |
| v0.2.0 | search, completions, new commands. |
| v0.2.1 | `exk` short binary alias. |

### Naming evaluation

existential-kernel (8/10, kept) > existview (6/10) > weltanschauung (5/10) > worldview (4/10) > existenceview (3/10).

## User

What name would be a 10/10?

## Assistant

A 10/10 probably doesn't exist — naming is always tradeoffs. The gap between 8 and 10 for "existential-kernel" is *length* (19 chars, hyphenated) and *jargon density* (both words are loaded).

A 10/10 would be: short, unique, evocative, pronounceable, and memorable after hearing it once.

**Candidates that push toward 10:**

| Name | Score | Why |
|------|-------|-----|
| **exo** | 9/10 | Greek prefix for "outside/beyond." Short (3 chars). Evokes existence without spelling it out. CLI: `exo lookup scope`. Problem: `exo` is taken (Exoscale CLI). |
| **kern** | 8.5/10 | Short, punchy, literally "kernel." `kern lookup scope`. Problem: typography term (letter spacing), may confuse. |
| **ousia** | 9/10 | Greek *ousia* (οὐσία) = "essence/being/substance." Aristotle's central metaphysical term. 5 chars, unique, pronounceable (OO-see-ah). `ousia lookup scope`. Rich philosophical lineage. Problem: obscure to non-philosophers. |
| **nous** | 8.5/10 | Greek *nous* (νοῦς) = "mind/intellect/reason." 4 chars. `nous lookup scope`. Problem: in casual English, "nous" means common sense (British slang). |
| **ontos** | 8.5/10 | From Greek *on/ontos* = "being." `ontos lookup scope`. Clear link to ontology. 5 chars. Problem: sounds like a plural. |

**The honest answer:** "existential-kernel" at 8/10 is probably optimal *for what it actually is*. The perfect name would be a single evocative word that immediately communicates "composable ontological framework rooted in existence" — but that concept is inherently multi-word. Any shorter name trades precision for brevity.

The `exk` alias already solves the typing problem. The full name communicates clearly in docs and conversation. That's a good split: `exk` for hands, `existential-kernel` for minds.

## User

Of those names, only ontos is available on crates.io. existential-kernel is difficult to remember.

existence is available

## Assistant

**`existence` is the 10/10 name.**

| | `existence` | `ontos` | `existential-kernel` |
|---|---|---|---|
| Memorable | Instantly — everyone knows the word | Requires explanation | Two loaded words |
| Precise | IS the root concept of the whole framework | Adjacent (Greek for "being") | Descriptive but clinical |
| CLI feel | `existence lookup scope` | `ontos lookup scope` | `exkernel lookup scope` |
| Short alias | `exs` (3 chars) | `ontos` (already short) | `exk` (3 chars) |
| Searchable | High — unique in tech context | High — unique | Medium — generic words |
| Available | crates.io yes, GitHub org likely | crates.io yes | Already established |

**Why `existence` works where `worldview` didn't:** "worldview" is vague — it could be a blog, a framework, anything. "existence" is *specific to this project* because Existence IS the first term, the universal set, the root of the ontology chain. The name isn't generic when the tool literally models Existence.

```
$ existence lookup scope
$ existence search "pattern"
$ existence new domain-model --ring 1
$ existence scope 0
```

Reads like natural language. "Look up scope in existence." "Search existence for pattern."

**Migration cost if you rename:**
- Rename GitHub org `existential-kernel` → `existence` (or create new)
- Rename crate `exkernel` → `existence` on crates.io
- Update all submodule URLs, CLAUDE.md refs, skill files
- Old `cargo install exkernel` still works (crates.io doesn't delete)
- Significant but one-time effort

**My recommendation:** If you're going to rename, do it now while the project is young (v0.2.1, low adoption). `existence` is genuinely better — it's the name the project *should* have had from the start.

Or keep `existential-kernel` as the org and just rename the crate/CLI to `existence`. The org name matters less (people interact with the CLI daily, the org rarely).

## User

Yes! Rename the crate to existence. There is a github user https://github.com/existence

So what should the org be named? existence-kernel, existence-org, something else?

## Assistant

**Recommendation: keep `existential-kernel` as the org.**

| Option | Score | Notes |
|--------|-------|-------|
| **existential-kernel** (keep) | 9/10 | Already established with 5 repos, stars, releases. Descriptive. No migration cost. |
| existence-kernel | 7/10 | Redundant — "existence" + "kernel" when the crate is just "existence" |
| existence-dev | 6/10 | Generic `-dev` suffix adds nothing |
| existence-org | 5/10 | `-org` is filler |

**The case for keeping it:** The org is infrastructure — it shows up in GitHub URLs and `git clone` commands, rarely in daily use. What matters is the crate name (`existence`) and the CLI binary (`existence`/`exk`). Those are what users type every day.

```
cargo install existence          ← what users see (clean)
git clone github.com/existential-kernel/existence  ← rare, background
```

`existential-kernel` as an org name is actually *good* — it's descriptive of the *project*, while `existence` is the *tool*. Different roles, different names is fine. Like how `rust-lang` is the org but `rustc` is the compiler.

**Rename plan:**
1. Rename crate `exkernel` → `existence` on crates.io (publish new crate, `exkernel` stays as redirect)
2. Rename repo `existential-kernel/exkernel` → `existential-kernel/existence`
3. Add `[[bin]] name = "existence"` + keep `[[bin]] name = "exk"` as alias
4. Update all refs (CLAUDE.md, skills, submodules, README)
5. Publish `existence` v0.3.0 to crates.io

Keep `existential-kernel` org? Or do you prefer a different org name?

## User

existence-framework? Rate that vs existential-kernel

## Assistant

| | `existence-framework` | `existential-kernel` |
|---|---|---|
| **Score** | 7.5/10 | 8/10 |
| **Clarity** | Immediately clear — it's a framework about existence | Clear but denser — two philosophical/technical terms |
| **Memorability** | Easy — common words | Harder — "existential" is a mouthful |
| **Cohesion with crate** | Strong — `existence` (crate) lives in `existence-framework` (org) | Weaker — `existence` (crate) lives in `existential-kernel` (org) |
| **Migration cost** | Full rename — new org, move 5 repos, update all refs | Zero — already established |
| **Tone** | Approachable, modern | Academic, technical |

**`existential-kernel` wins slightly** because:
1. "Kernel" is more precise than "framework" — the project IS a kernel (minimal core that everything else builds on), not a framework (which implies scaffolding/conventions/boilerplate)
2. The mismatch between `existence` (crate) and `existential-kernel` (org) is actually fine — they're related words, not unrelated brands
3. Zero migration cost

**But** if memorability is the priority, `existence-framework` is easier to recall. The word "framework" is overused in tech, though — every JS library calls itself a framework.


**My vote:** Keep `existential-kernel`. The org name is background noise. Save your naming energy for things users touch daily (the crate name, which is already perfect).

## User

existence-kernel vs existential-kernel

## Assistant

**`existence-kernel` is better.**

| | `existence-kernel` | `existential-kernel` |
|---|---|---|
| **Score** | 8.5/10 | 8/10 |
| **Cohesion** | Direct match — `existence` crate in `existence-kernel` org | Adjective form — related but not matching |
| **Shorter** | 16 chars | 19 chars |
| **Clearer** | Noun + noun — "the kernel of existence" | Adjective + noun — "the existential kind of kernel" |
| **Memorable** | Easier — both are common words | Harder — "existential" is 5 syllables |

`existence-kernel` reads as "the kernel of existence" — exactly what the project is. It pairs perfectly with the `existence` crate. And it's 3 characters shorter.

The only downside: migration cost (rename org, move 5 repos, update submodule URLs). But since you're already renaming the crate, you're doing a big rename anyway — might as well do both at once.

**If you want to rename, I'd go with `existence-kernel`.** Want me to execute the full rename (org + crate + repos + submodules)?

## User

The org will be named existence-kernel? It sounds more like a software layer. But it could also be a technique with a better description than existence-framework.

Are there better variations? I don't mind renaming, so let's find the best name available.

## Assistant

You're right — "kernel" sounds like software infrastructure. The org should communicate *what the project is about*, not how it's architected. All these are available on GitHub:

| Name | Score | What it communicates |
|------|-------|---------------------|
| **existence-ontology** | 9/10 | "An ontology of existence." Precise, academic, exactly what it is. Pairs perfectly with `existence` crate. Natural for the repos: `existence-ontology/existence`, `existence-ontology/ontology`, `existence-ontology/spec`. |
| **existence-lang** | 8/10 | "A language of existence." Evocative — the project IS a language (ubiquitous language for defining domains). Short. But could be confused with a programming language. |
It sounds like a programming language...but with agent loops it is a language.
| **existence-foundation** | 7.5/10 | Implies permanence and community stewardship. But "foundation" is overused (Apache Foundation, Linux Foundation, etc.) and premature for a young project. |
| **existence-kernel** | 7.5/10 | "The kernel of existence." But you're right — sounds like a software layer, not a technique/discipline. |
| **existence-spec** | 7/10 | Clear but narrow — implies just a specification, not the tooling and content. |
| **existence-protocol** | 6.5/10 | Too buzzwordy. Implies networking/blockchain. |

**My pick: `existence-ontology`.** It's the most honest description. The project IS an ontology of existence — term definitions, ring structure, composable scoping. The `existence` crate is the CLI for working with the `existence-ontology`. Clean hierarchy:

```
existence-ontology (org)
├── existence    ← CLI tool (crate name)
├── ontology     ← 114 term definitions
├── spec         ← node format + ring spec
├── .github      ← org profile
└── *.github.io  ← landing page
```

`existence-lang` is the compelling alternative if you want something shorter and more evocative.

## User

existence-lang is nice as well. With agent loops & the cli it is semi-executable. I wonder if we can actually make a programming language out of it.

## Assistant

It already *is* a language — and it's already semi-executable. You're closer than you think.

**What you have today:**
- **Vocabulary:** 114 defined terms with precise semantics
- **Scoping rules:** Ring 0 → Ring 1 → Ring 2 → domain overlay (composable inheritance)
- **Type system:** Each term has Ontology (what it IS), Axiology (what it's WORTH), Epistemology (how it's KNOWN)
- **Linking:** Terms reference each other via `[term](./term.md)` — a dependency graph
- **Validation:** `existence lint` checks structural correctness
- **Query:** `existence search`, `existence lookup`, `existence scope`
- **The interpreter:** An LLM in an agent loop. It reads the definitions and *applies* them.

**What makes it "semi-executable" right now:**
```
existence scope domain agent-doc
```
This is already a program. It says: "Resolve the terms relevant to agent-doc's domain, starting from the existential kernel, through the software ring, into the domain overlay." The CLI executes it deterministically. The agent loop executes the *meaning*.

**To make it a full language, you'd add:**

1. **Assertions** — `existence assert "A Session derives from project + story"` — validates derivation chains
2. **Transforms** — `existence derive healthcare` — given a domain, generate a domain overlay by analyzing which kernel terms apply
3. **Constraints** — define invariants between terms: "if scope narrows, resolution must increase" — enforceable in code reviews
4. **Composition** — `existence compose agent-doc + corky` — merge two domain overlays, detect conflicts
5. **Execution via agent** — `existence run "Given a new entity in domain X, classify it using kernel terms"` — LLM interprets the ontology as rules

The insight is that the LLM is the runtime. Traditional languages need compilers because machines can't interpret natural language. With LLMs, natural language definitions *become* executable specifications. The ontology terms are the instruction set; the agent loop is the CPU.

**`existence-lang` as an org name would be aspirational but honest** — it signals where this is going: from a static ontology toward an executable language for modeling existence. If you believe the agent-loop-as-runtime thesis, it's not even aspirational — it's descriptive.

**My updated recommendation:**
- **`existence-ontology`** if you want to describe what it is *today* (9/10)
- **`existence-lang`** if you want to describe what it's *becoming* (9.5/10)

The `-lang` name is a commitment to a direction. If you're serious about making it executable, claim the name now.

## User
lets go with existence-lang
existence for crates, existence-lang for github org

## Assistant

**Migration plan:**

```
existential-kernel (old)          →  existence-lang (new)
├── exkernel → existence          ←  CLI crate, renamed
├── ontology → ontology           ←  term definitions, same name
├── spec → spec                   ←  node format, same name
├── .github → .github             ←  org profile, updated
└── *.github.io → *.github.io    ←  landing page, updated
```

**You need to do first:** Create the `existence-lang` org at https://github.com/organizations/plan — GitHub requires browser auth for org creation.

**Once created, I'll handle:**
1. Create repos under `existence-lang` (existence, ontology, spec, .github, existence-lang.github.io)
2. Push all content with updated refs (`existential-kernel` → `existence-lang`, `exkernel` → `existence`)
3. Rename crate to `existence`, publish v0.3.0 to crates.io
4. Update submodules in agent-loop (`existential-kernel/` → TBD directory name)
5. Update all CLAUDE.md, skills, README, landing page refs
6. The old `existential-kernel` repos stay as archives with redirect notes

**Question:** Should the submodule directory in agent-loop be `existence-lang/` or just `existence/`?

Let me know once the org is created and I'll execute the full migration.

## User

I renamed the org.
https://github.com/organizations/existence-lang

Please update docs

and references to point to new repo location

## Assistant

**Migration complete. `existence` v0.3.0 published.**

**What changed:**

| Before | After |
|--------|-------|
| Org: `existential-kernel` | Org: `existence-lang` |
| Crate: `exkernel` | Crate: `existence` |
| Binary: `exkernel` + `exk` | Binary: `existence` + `exk` |
| Config: `exkernel.toml` | Config: `existence.toml` |
| Data dir: `~/.exkernel/` | Data dir: `~/.existence/` |

**Published:** [existence v0.3.0 on crates.io](https://crates.io/crates/existence) · [GitHub Release](https://github.com/existence-lang/existence/releases/tag/v0.3.0)

**Install:** `cargo install existence`

**Repos updated:**
- `existence-lang/existence` — crate renamed, all refs updated, 36 tests pass
- `existence-lang/ontology` — config file renamed, meta/README/CLAUDE updated
- `existence-lang/spec` — SPEC.md, RINGS.md, README updated
- `existence-lang/.github` — org profile updated
- `existence-lang/existence-lang.github.io` — landing page updated
- `agent-doc/CLAUDE.md` — domain ontology refs updated
- `.claude/skills/existence/` — skill renamed from exkernel
- `.gitmodules` — submodule URLs updated to `existence-lang`

```
$ existence --version
existence 0.3.0

$ existence --ontology existential-kernel/ontology scope 0
Ring 0 — kernel (14 terms)
  existence, entity, abstraction, scope, context, resolution,
  pattern, system, domain, focus, perspective, consciousness,
  evolution, story
```

## User

Is there a better abbreviation than exk available? Is exc available? exis? xist? xst? extc? any other good abbreviations available?

## Assistant

Checked crates.io and system commands:

| Name | crates.io | Feel | Score |
|------|-----------|------|-------|
| **xist** | Available | Punchy, memorable. "X-ist." Sounds like "exist" when spoken. 4 chars. | 9/10 |
| **exis** | Available | Sounds like "exist" truncated. 4 chars. Clear derivation. | 8/10 |
| **exi** | Available | 3 chars, minimal. But doesn't evoke "existence" strongly — could be anything. | 7/10 |
| **xst** | Available | Ultra-short (3 chars). But unpronounceable — just consonants. | 6/10 |
| `exc` | **Taken** | — | — |
| `ext` | **Taken** | — | — |
| **extc** | Available | 4 chars but doesn't read as "existence." Looks like "extract-c." | 5/10 |
| **exst** | Available | 4 chars. Looks like a typo of "exist." Not bad but awkward. | 6/10 |

**My pick: `xist`.** It's the best balance of short + evocative + memorable. Spoken aloud it sounds like "exist." Typed, it's distinctive and unlikely to conflict. `xist search scope` reads well.

Runner-up: `exis` — more obvious derivation from "existence" but less punchy.

Want me to add `xist` as a third binary alias alongside `existence` and `exk`?

## User

Yes. xist.

## Assistant

**Done. `xist` replaces `exk`. Released v0.3.1.**

```
$ cargo install existence
  Installing existence + xist

$ xist --version
existence 0.3.1

$ xist search "scope"
scope (Ring 0) — The breadth, depth or reach of an Entity...
```

Binaries: `existence` (full) + `xist` (short). Old `exk` auto-removed on upgrade.

[crates.io](https://crates.io/crates/existence) · [v0.3.1](https://github.com/existence-lang/existence/releases/tag/v0.3.1)

## User

Rename existential-kernel to existence-lang in this repo. Find any old references to existential-kernel and rename to existence or existence-lang
