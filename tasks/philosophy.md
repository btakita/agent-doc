---
agent_doc_session: cb5672cc-5d1d-4228-af30-640a336f692e
tmux_session: claude
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
