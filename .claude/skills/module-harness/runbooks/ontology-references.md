# Ontology References in Module Specs

When writing or auditing module specs, cross-reference terms from the project's domain ontology. This ensures module documentation uses consistent vocabulary and links to canonical definitions.

## When to Apply

- During `module-harness apply <file>` — check SPEC.md and README.md domain ontology for terms that apply to the module
- During `module-harness audit <file>` — flag undefined terms used in specs that should link to ontology nodes
- During `module-harness seed <file>` — use ontology terms when bootstrapping specs from implementation

## How to Reference

**Only reference ontology defined within the project or its dependencies.** Source code can be cloned into environments that don't have the existence kernel or other local-only paths. All ontology links in source code must resolve within the project tree.

### In Spec sections

When a spec entry uses a domain ontology term, reference the project's domain ontology (README.md):

```rust
//! ## Spec
//! - `classify_diff(diff_text)` classifies a unified diff into a
//!   [DiffType](README.md#interaction-model) including Directives (approval signals).
```

### In Agentic Contracts

When a contract references domain behavior, use the term name and reference the project ontology:

```rust
//! ## Agentic Contracts
//! - A Directive (approval signal) authorizes execution at full quality.
//!   The directive's brevity does not reduce expected thoroughness.
//!   See [Interaction Model](README.md#interaction-model).
```

### What NOT to do

- Do NOT link to the existence kernel or any local-only path in source code (not portable)
- Do NOT link to absolute paths outside the project tree
- Do NOT assume any path that isn't checked into the repo or declared as a dependency

## Ontology Sources (in priority order)

1. **Project domain ontology** — `README.md ## Domain Ontology` (e.g., Binding, Reconciliation, Directive)
2. **SPEC.md** — project-level invariants that define behavioral terms
3. **Existence kernel** — referenced by name only in source code; full definitions are agent-local and are NOT linked from source

## Audit Checklist

When auditing a module's harness:

- [ ] Terms in Spec match terms in the project domain ontology (README.md)
- [ ] Ontology terms that apply to this module are referenced (not just used implicitly)
- [ ] All links resolve within the project tree (no absolute paths, no `~/` references)
- [ ] Ontology nodes referenced in README.md actually exist (verify file/section)
- [ ] No ontology terms are used as exclusive labels (Universal Applicability Principle)

## Example (HEAD)

For `diff.rs`, the spec should reference terms from `README.md#interaction-model`:
- **Directive** — `classify_diff` classifies directives as `DiffType::Approval`
- **Annotation** — inline edits classified as `DiffType::Annotation`
- **Cycle** — the diff is computed once per cycle
- **Diff** — the module's core responsibility
