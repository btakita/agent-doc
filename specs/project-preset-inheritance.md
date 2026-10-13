# Project Preset Inheritance

## Decision

Project-wide prompt presets live in the YAML frontmatter of
`.agent-doc/project.md`. Session documents resolve that file dynamically; they
do not copy or synchronize its preset map. This gives existing documents new
project presets as soon as the project file changes while keeping one canonical
source.

The initial inheritance surface is deliberately limited to `presets` (the
`prompt_presets` alias remains accepted). Other document frontmatter does not
implicitly become project policy.

```markdown
---
presets:
  "#review": Review the change and run focused tests.
  "#release":
    prompt: Prepare and publish the release.
    runbook: runbooks/release.md
---

# Agent-doc project defaults

This body may explain the project's presets, but is not inherited.
```

## Resolution

For a session document under a discovered project root:

1. Read only the frontmatter of the nearest `.agent-doc/project.md`.
2. Parse its presets through the same `Frontmatter` / `PromptPresets` path used
   by session documents, including scalar, multiline, and structured values.
3. Treat project presets as defaults and overlay the session document's
   declarations by exact key. A document-local declaration wins, including an
   unset, half-typed declaration; it must mask an inherited value rather than
   execute the project value while the operator is editing an override.
4. Preserve the source preset's diagnostics and structured `runbook` metadata.
   Project-relative runbook paths continue to resolve from the project root.
5. Resolve on read (or invalidate a cache by project-file identity and content
   change). Never write inherited entries into a session document.

A missing project file means there are no project defaults. An existing file
with malformed frontmatter must produce an actionable diagnostic and must not
silently authorize an inherited preset. Document-local presets remain the
document's own declarations.

## Why not `.agent-doc/project.yaml`

A standalone YAML file would duplicate the serialization already embedded in
Markdown frontmatter while introducing a second project-config discovery and
migration path. It would also make the boundary with `.agent-doc/config.toml`
unclear: `config.toml` is the typed operational configuration surface, whereas
shared presets use the document frontmatter schema and its preservation rules.

`project.md` keeps the same syntax and parser as session documents, supports an
explanatory Markdown body, preserves structured preset/runbook behavior, and
can later host other explicitly whitelisted shared frontmatter without adding
another file format. A project template for creating new session documents is
orthogonal: templates may seed document-owned fields, but must not materialize
inherited presets.

## Required coverage

Integration coverage for the resolver must prove:

- a project-only scalar, multiline, and structured preset resolves;
- a document-local value overrides the same project key;
- a document-local unset declaration masks the same project key;
- edits to `project.md` affect an existing document without rewriting it;
- missing project files yield an empty inherited map;
- malformed project frontmatter cannot silently fall back to stale or partial
  inherited authorization; and
- consumers such as preflight expansion and queue subagent authorization use
  the same effective preset map.
