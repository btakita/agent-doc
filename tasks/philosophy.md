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
