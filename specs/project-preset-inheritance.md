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

## User setup contract

After `agent-doc init` establishes a project root, the user defines shared
presets by creating or editing `<project-root>/.agent-doc/project.md`; they do
not put prompt presets in `.agent-doc/config.toml`. The project file may contain
only YAML frontmatter or may include an explanatory Markdown body. Preset names
that start with `#` must be quoted so YAML does not parse them as comments.

Both `presets:` and `prompt_presets:` are valid map keys. A scalar map value is
the prompt body. A structured value uses `prompt:` plus an optional `runbook:`
whose relative path is anchored at the project root. Users invoke a shared
preset exactly as they invoke a document-local preset: `#name` in prompt text,
`preset name`, or the corresponding queue reference.

## Resolution

For a session document under one or more nested project roots:

1. Walk from the nearest discovered project root through its ancestors, bounded
   by `AGENT_DOC_ROOT_CEILING_DIRECTORIES`, and read every existing
   `.agent-doc/project.md`.
2. Parse their presets through the same `Frontmatter` / `PromptPresets` path used
   by session documents, including scalar, multiline, and structured values.
3. Treat farther ancestors as defaults, overlay nearer project declarations by
   exact key, then overlay the session document's declarations. A nearer or
   document-local declaration wins, including an unset, half-typed declaration;
   it must mask an inherited value rather than execute the ancestor value while
   the operator is editing an override.
4. Preserve the source preset's diagnostics and structured `runbook` metadata.
   Project-relative runbook paths continue to resolve from the project root.
5. Resolve on read (or invalidate a cache by project-file identity and content
   change). Never write inherited entries into a session document.
6. After overlaying document declarations, recursively interpolate references
   in every effective preset body. A bare hashtag token is substituted only
   when that exact key is declared in the effective map, so ordinary hashtags
   remain prose. `{{#name}}` is the explicit form: it requires `#name` to be
   declared. Missing explicit references, declared-but-unset values, malformed
   values, and cycles fail closed before a preset can authorize work.

For example, the project may define a reusable preset and defaults:

```yaml
presets:
  "#generic-preset": "Check JIRA tickets for #project on {{#board}}"
  "#project": DEFAULT
  "#board": General
```

A session document can override only the values:

```yaml
presets:
  "#project": AGENT-DOC
  "#board": Platform
```

Invoking `#generic-preset` then expands to `Check JIRA tickets for AGENT-DOC
on Platform`. Expansion is a read-time projection: the project and session
frontmatter remain byte-for-byte operator-owned, and editing `project.md` does
not require restarting agent-doc. Structured `runbook` metadata remains attached
to the preset that declares it; interpolation composes prompt text only.

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
- a nearer nested project inherits parent-only presets and overrides shared keys;
- a nearer nested-project unset declaration masks the same parent key;
- a document-local value overrides the same nested or parent project key;
- a document-local unset declaration masks the same nested or parent project key;
- edits to `project.md` affect an existing document without rewriting it;
- missing project files yield an empty inherited map;
- malformed project frontmatter cannot silently fall back to stale or partial
  inherited authorization;
- consumers such as preflight expansion and queue subagent authorization use
  the same effective preset map;
- project presets interpolate document-local overrides recursively without
  rewriting either document; and
- missing explicit references, unset declared values, and cycles fail closed,
  while undeclared ordinary hashtags remain literal text.
