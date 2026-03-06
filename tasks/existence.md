---
agent_doc_session: cb5672cc-5d1d-4228-af30-640a336f692e
tmux_session: '1'
---



## Status Summary (compacted 2026-03-04)

### Existence Lang — Composable Ontology Framework

**Org:** [existence-lang](https://github.com/existence-lang) (renamed from existential-kernel)
**Crate:** [existence](https://crates.io/crates/existence) v0.3.1 (renamed from exkernel)
**Binaries:** `existence` (full) + `xist` (short alias)
**Config:** `existence.toml`
**Data dir:** `~/.existence/`

### Repos (all submodules in `agent-loop/existence-lang/`)

| Repo | Description |
|------|-------------|
| [existence](https://github.com/existence-lang/existence) | Rust CLI v0.3.1. Binaries: `existence` + `xist`. 36 tests. |
| [ontology](https://github.com/existence-lang/ontology) | 114 term files across 3 rings, lint-clean. `existence.toml` config. |
| [spec](https://github.com/existence-lang/spec) | SPEC.md node format + RINGS.md ring structure. |
| [.github](https://github.com/existence-lang/.github) | Org profile README. |
| [existence-lang.github.io](https://existence-lang.github.io) | Landing page. |

### Ring Structure

- **Ring 0 (Kernel, 14 terms):** Universal existential scope, always loaded — existence, entity, abstraction, scope, context, resolution, pattern, system, domain, focus, perspective, consciousness, evolution, story
- **Ring 1 (Software, 17 terms):** DDD bridge — project, model, algorithm, state, type, definition, information, signal, language, tool, environment, coherence, communication, collective, integrity, qualitative*, quantitative* (*pending)
- **Ring 2 (Extended, 85 terms):** Philosophy, spirituality, cognition, linguistics

### CLI Commands (v0.3.1)

| Command | Status |
|---------|--------|
| `lookup <term>` | Done — read node definition, `--json` output |
| `search <query>` | Done — relevance scoring, `--json`, `--limit` |
| `scope [ring]` | Done — list terms by ring |
| `lint [path]` | Done — validate sections + links |
| `graph [ring]` | Done — DOT/JSON relationship graphs |
| `fetch [source]` | Done — clone/pull to `~/.existence/sources/` |
| `new <term>` | Done — scaffold from SPEC.md template |
| `completions <shell>` | Done — bash/zsh/fish/elvish/powershell |
| `install` / `serve` / `build-site` / `context` | Stubs |

### Philosophy Integration

- `philosophy/install.sh` — symlinks `~/.claude/philosophy` → git repo, writes 14-term kernel to `~/.claude/CLAUDE.md`
- Domain overlays: repos extend base ontology via `## Domain Ontology` in their CLAUDE.md
- `/existence` Claude skill at `.claude/skills/existence/SKILL.md` — supports lookup, search, new (interactive), lint, scope

### DDD Mapping

Ubiquitous Language = language + domain; Bounded Context = scope + context; Entity = entity; Aggregate = system; Domain Event = evolution + signal; Model = model; Value Object = state + type.

### Naming History

existential-kernel → existence-lang (org), exkernel → existence (crate), exk → xist (short alias). Rename completed in v0.3.0–v0.3.1.

### Key Decisions

- **`existence-lang`** org: signals the project is evolving toward an executable language (LLM as runtime, ontology terms as instruction set)
- **`xist`** short alias: punchy, sounds like "exist" spoken aloud, 4 chars
- **Navigation:** Links to upstream, not copies. Build-time resolution, display-time linking.
- **Ring system:** Composable scoping — Ring 0 always loaded, higher rings opt-in
- **Four traditions synthesized:** Process philosophy (Whitehead), phenomenology (Husserl), information-theoretic ontology, existentialism

### Releases

| Version | What shipped |
|---------|-------------|
| v0.1.0 | Initial: lookup, scope, lint, graph, fetch. 114 terms. |
| v0.2.0 | search, completions, new commands. |
| v0.2.1 | `exk` short binary alias. |
| v0.3.0 | Renamed to `existence` crate + `existence-lang` org. |
| v0.3.1 | `xist` replaces `exk` as short alias. |

### Pending

- `install` / `serve` / `build-site` / `context` stubs
- Executable language features: assertions, transforms, constraints, composition
- Static JSON site generation (GitHub Pages)
- Submodule internals still reference `existential-kernel` in test fixtures

## User

~/work/btakita/agent-loop/philosophy master                                                                                                                                                                                            19:33:46
❯ gp
Username for 'https://github.com': ^C

