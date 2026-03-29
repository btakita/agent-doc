---
agent_doc_session: cb5672cc-5d1d-4228-af30-640a336f692e
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---

## Existence Lang — Composable Ontology Framework

**Org:** [existence-lang](https://github.com/existence-lang) | **Crate:** [existence](https://crates.io/crates/existence) v0.3.1 | **Binaries:** `existence` + `xist`
**Config:** `existence.toml` | **Data:** `~/.existence/`

### Repos

| Repo | Description |
|------|-------------|
| [existence](https://github.com/existence-lang/existence) | Rust CLI v0.3.1. 36 tests. |
| [ontology](https://github.com/existence-lang/ontology) | 114 terms, 3 rings, lint-clean. |
| [spec](https://github.com/existence-lang/spec) | SPEC.md + RINGS.md |
| [.github](https://github.com/existence-lang/.github) | Org profile README |
| [existence-lang.github.io](https://existence-lang.github.io) | Landing page |

### Rings

- **R0 (Kernel, 14):** existence, entity, abstraction, scope, context, resolution, pattern, system, domain, focus, perspective, consciousness, evolution, story
- **R1 (Software, 17):** project, model, algorithm, state, type, definition, information, signal, language, tool, environment, coherence, communication, collective, integrity, qualitative*, quantitative*
- **R2 (Extended, 85):** Philosophy, spirituality, cognition, linguistics

### CLI (v0.3.1)

Done: `lookup`, `search`, `scope`, `lint`, `graph`, `fetch`, `new`, `completions`
Stubs: `install`, `serve`, `build-site`, `context`

### Integration

- `philosophy/install.sh` — symlinks + writes kernel to `~/.claude/CLAUDE.md`
- Domain overlays via `## Domain Ontology` in repo CLAUDE.md files
- `/existence` Claude skill — lookup, search, new, lint, scope

### DDD Mapping

Ubiquitous Language = language + domain; Bounded Context = scope + context; Entity = entity; Aggregate = system; Domain Event = evolution + signal; Model = model; Value Object = state + type.

### Key Decisions

- `existence-lang` org — evolving toward executable language (LLM runtime, ontology as instruction set)
- `xist` short alias — punchy, 4 chars
- Links to upstream, not copies; build-time resolution
- Ring system: R0 always loaded, higher rings opt-in
- Four traditions: process philosophy, phenomenology, info-theoretic ontology, existentialism

### Releases

v0.1.0 (lookup/scope/lint/graph/fetch) → v0.2.0 (search/completions/new) → v0.2.1 (`exk`) → v0.3.0 (rename) → v0.3.1 (`xist`)

### Pending

- Stub commands: install, serve, build-site, context
- Executable language features: assertions, transforms, constraints, composition
- Static JSON site generation (GitHub Pages)
- Test fixtures still reference `existential-kernel`

<!-- agent:exchange -->
Converted to template mode and compacted. Status summary reduced from ~110 lines to ~50. Use the input component above for future prompts.
<!-- /agent:exchange -->
