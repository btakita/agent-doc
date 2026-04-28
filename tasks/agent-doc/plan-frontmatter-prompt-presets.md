# Plan: Frontmatter Prompt Presets for Orchestrated Tasks

**Pending:** `#fppr` in `tasks/agent-doc/agent-doc-bugs.md`
**Status:** Draft

## Problem

`agent-doc orchestrate` does not currently have a binary-owned way to apply the
same extra prompt text to every task in a batch. Users can sometimes approximate
this by placing shared instructions in the session document, but that behavior
depends on the orchestration mode and on prompt assembly details rather than on
an explicit contract.

That leaves a few gaps:

- no first-class place to define reusable batch-level prompt text
- no deterministic syntax for selecting one of several reusable presets
- no guarantee that `parallel` orchestration sees the same shared guidance as
  `sequential` / `dag`
- no structured preflight signal that a preset must be expanded before dispatch

## Goal

Add a binary-owned prompt-preset flow so a session document can define reusable
frontmatter presets and invoke them from the exchange with syntax such as
`preset #1`, with the selected preset expanded into the prepared task prompt as
annotated text such as `(preset #1)`.

## Non-Goals

- replacing existing project-wide instruction files or harness defaults
- turning `preflight` into a free-form prompt composer
- supporting arbitrary templating or nested preset expansion in the first pass

## Desired Contract

### Frontmatter definition

The document can define one or more named prompt presets in frontmatter. Exact
syntax is still open, but it should be deterministic and easy to parse.

Example shape:

```yaml
prompt_presets:
  "#1": |
    Today is 2026-04-25.
    Be concise and keep the work tree clean.
  release-check: |
    Run the full verification suite before finalize.
```

### Invocation

The changed exchange can request one or more presets with lightweight prose such
as `preset #1` or `preset release-check`.

### Preflight responsibility

`agent-doc preflight` should detect requested preset references, validate that
the referenced preset exists, and emit a structured field for downstream
dispatch. It should not inline the full preset body into the diff output.

### Prompt preparation responsibility

The prompt-assembly step should expand the selected preset text into the actual
task prompt with a visible label, for example:

```text
(preset #1)
Today is 2026-04-25.
Be concise and keep the work tree clean.
```

That expansion path should be shared by `sequential`, `dag`, and `parallel`
orchestration so the behavior does not depend on mode-specific prompt plumbing.

## Design Options

### Option A: Frontmatter text presets + preflight references

Store preset bodies directly in frontmatter, have preflight emit referenced
preset names, and expand them during prompt preparation.

- **Pro:** Single-document workflow, deterministic, easy to reason about.
- **Con:** Large presets make frontmatter noisy.

### Option B: Frontmatter preset map to external files

Frontmatter maps preset names to file paths, and prompt preparation reads those
files when a preset is requested.

- **Pro:** Better for long prompts and shared reuse across documents.
- **Con:** More I/O, more failure cases, and weaker document portability.

### Option C: Skill-only prose convention

Document the phrase `preset #1` in the skill and rely on the model to read
frontmatter and inline the preset manually.

- **Pro:** Fastest to prototype.
- **Con:** Repeats the same reliability problem this repo has been moving away
  from; not binary-owned enough for the intended contract.

## Recommended Approach

Start with **Option A**.

The first version should keep the whole feature self-contained in the session
document:

- frontmatter owns the preset definitions
- preflight owns preset-reference detection and validation
- prompt preparation owns expansion into the concrete task prompt

That keeps the feature deterministic, testable, and mode-independent without
adding external file resolution in the first pass.

## Proposed Structured Output

If a preset is requested, preflight should emit a compact field such as:

```json
{
  "prompt_presets_requested": ["#1"]
}
```

If a requested preset is missing, preflight should fail closed with a concrete
error instead of silently skipping the reference.

## Implementation Steps

- [ ] 1. Define the frontmatter schema for prompt presets and document the exact
      invocation grammar for `preset <name>`
- [ ] 2. Extend frontmatter parsing so preset definitions round-trip cleanly
- [ ] 3. Teach `preflight` / diff analysis to detect preset requests and emit a
      structured field for downstream consumers
- [ ] 4. Update prompt preparation so selected presets expand into the concrete
      task prompt with a visible `(preset <name>)` label
- [ ] 5. Route that prompt expansion through every orchestration mode,
      especially `parallel`, so shared guidance stays consistent
- [ ] 6. Add regression coverage for valid preset expansion, missing preset
      errors, repeated preset invocation, and mode parity across orchestration
- [ ] 7. Update skill/runbook/spec surfaces so the feature is documented as a
      binary-owned contract rather than model-side convention

## Files Likely to Change

- `src/agent-doc/src/frontmatter.rs`
- `src/agent-doc/src/preflight.rs`
- `src/agent-doc/src/diff.rs`
- `src/agent-doc/src/orchestrate.rs`
- `src/agent-doc/src/prepare_prompt.rs` or the current prompt-assembly surface
- `src/agent-doc/SKILL.md`
- `src/agent-doc/specs/07-commands.md`

## Open Questions

- Should preset names be limited to simple identifiers, or should symbolic names
  like `#1` be preserved exactly?
- Should multiple preset invocations compose in-order within one task batch?
- Should ordinary non-orchestrate response cycles also support preset expansion,
  or is this strictly an orchestration feature in the first version?
- Is prompt expansion better modeled as part of `preflight`, `prepare prompt`,
  or a shared lower-level prompt builder used by both surfaces?
