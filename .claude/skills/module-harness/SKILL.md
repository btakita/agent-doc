---
description: Generate, update, and audit module-level harness context (spec, contracts, evals) for any programming language
user-invocable: true
argument-hint: "<file|seed file|apply file|batch dir|audit dir>"
---

# module-harness

Generate and manage module-level harness context — structured doc comments containing Spec, Agentic Contracts, and Evals for each source module.

## Invocation

```
/module-harness <file>          — audit: read module doc + SPEC.md, flag deviations (DEFAULT)
/module-harness seed <file>     — bootstrap spec from implementation (explicit opt-in)
/module-harness apply <file>    — read SPEC.md, propose module-level spec additions/removals/transfers
/module-harness batch <dir>     — audit all modules in a directory
/module-harness audit <dir>     — report coverage and drift (alias for batch audit)
/module-harness parse <file>    — parse existing harness to JSON (uses binary)
/module-harness inventory <dir> — list all evals across modules (uses binary)
```

## Command Semantics

### Default: Audit (read-only)

`/module-harness <file>` — the default operation is **audit**, not generation.

1. Read the module's existing doc comment (Spec, Contracts, Evals)
2. Read the project's SPEC.md (if present) for authoritative invariants
3. Compare implementation against spec — flag deviations:
   - Functions in code but not in spec
   - Spec entries that don't match implementation behavior
   - Contracts that are violated by the code
   - Missing evals for covered scenarios
4. Report findings without modifying the file

**Why audit is default:** Generating spec from implementation risks codifying bugs as spec. The user should write the spec first (in SPEC.md), then use module-harness to validate the implementation matches.

### Seed: Bootstrap from Implementation

`/module-harness seed <file>` — explicitly derive spec from what the code does.

1. Read the source file
2. Analyze all public functions, types, and exports
3. Generate Spec, Contracts, and Evals from implementation behavior
4. Write the doc comment at the top of the file
5. Verify the file still compiles/parses

**When to use:** Only for modules with NO existing spec. Once a spec exists, use `audit` or `apply` instead.

### Apply: Spec-First Workflow

`/module-harness apply <file>` — read SPEC.md and propose module-level changes.

1. Read the project's SPEC.md for authoritative invariants and domain ontology
2. Read the module's current doc comment
3. Propose:
   - Spec entries to **add** (from SPEC.md invariants that apply to this module)
   - Spec entries to **remove** (that contradict SPEC.md)
   - Spec entries to **transfer** to another module (misplaced responsibility)
4. Present changes for approval before writing

**When to use:** After writing or updating SPEC.md with new invariants/ontology.

### Batch and Audit

`/module-harness batch <dir>` — run audit on all source files in directory.
`/module-harness audit <dir>` — same as batch (alias). See `runbooks/module-harness-audit.md`.

## Harness Format

The harness is a structured doc comment at the top of each source file:

**Rust (`//!`):**
```rust
//! # Module: <name>
//!
//! ## Spec
//! - <behavioral specification for each public function/type>
//!
//! ## Agentic Contracts
//! - <invariants and guarantees for both AI agents and API consumers>
//!
//! ## Evals
//! - <eval_name>: <scenario> → <expected outcome>
```

**Python (docstring):**
```python
"""
# Module: <name>

## Spec
- <behavioral specification>

## Agentic Contracts
- <invariants and guarantees>

## Evals
- <eval_name>: <scenario> → <expected outcome>
"""
```

**TypeScript/JavaScript/Kotlin/Java (JSDoc/KDoc):**
```typescript
/**
 * # Module: <name>
 *
 * ## Spec
 * - <behavioral specification>
 *
 * ## Agentic Contracts
 * - <invariants and guarantees>
 *
 * ## Evals
 * - <eval_name>: <scenario> → <expected outcome>
 */
```

## Binary (`module-harness`)

Located at `src/module-harness/`. Provides deterministic operations:

- `module-harness parse <file>` — extract harness to JSON
- `module-harness inventory <dir>` — list all evals across modules
- `module-harness diff <file>` — show harness summary

## Guidelines

- **Spec should be complete**: every public function/type gets an entry
- **Contracts focus on guarantees**: what callers can assume without reading implementation
- **Evals are named**: use snake_case names that could be test function names
- **Map to existing tests**: when a `#[test]` or test function matches an eval scenario, note it
- **Keep concise**: one line per entry in Spec and Contracts; name + scenario for Evals
- **Language-agnostic format**: the section structure is identical across languages; only the comment syntax changes
- **SPEC.md is authoritative**: when SPEC.md and module doc disagree, SPEC.md wins

## Runbooks

- `audit harness` — [runbooks/module-harness-audit.md](runbooks/module-harness-audit.md)
- `ontology references` — [src/module-harness/runbooks/ontology-references.md](../../../src/module-harness/runbooks/ontology-references.md)
