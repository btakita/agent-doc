# Project Preset Inheritance

## Decision

Project-wide frontmatter defaults live in the YAML frontmatter of
`.agent-doc/project.md`. Session documents resolve that file dynamically; they
do not copy or synchronize inherited values. This gives existing documents new
project configuration as soon as the project file changes while keeping one
canonical source. Every field accepted by document frontmatter is accepted as
a project default, including `agent`, model selection, environment, guards, and
prompt presets (`prompt_presets` and its `presets` alias).

```markdown
---
presets:
  "#review": Review the change and run focused tests.
  "#release":
    prompt: Prepare and publish the release.
    runbook: runbooks/release.md
---

# Agent-doc project defaults

This body may explain the project's presets. Preset prose in the body is not
inherited. `agent:rules` blocks in this nearest project file are inherited as
turn context; see the document-format spec.
```

For ordinary fields, precedence is document frontmatter, nearest project.md,
farther ancestor project.md, then any corresponding config.toml/global/built-in
fallback. A top-level declaration replaces the inherited field as a unit, and
an explicit empty list or map masks the project value. Prompt presets are the
exception: their established merge-by-name behavior is retained.

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
2. Parse every field through the same typed `Frontmatter` path used by session
   documents, including aliases and explicit empty collections.
3. Treat farther ancestors as defaults, overlay nearer project declarations by
   canonical top-level key, then overlay the session document's declarations.
   Prompt presets overlay by exact preset name; an unset, half-typed preset
   declaration masks an inherited value rather than executing the ancestor
   value while the operator is editing an override.
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

### Queue heads that invoke an inherited preset

A queue head whose leading token is an effective preset (`- #review`,
`- #gh-fix <url>`, `do [#review]`) is a preset invocation: free-text work the
preset expands, not a reference to tracked work. This holds whether the preset
is declared in the session document or only inherited from a
`.agent-doc/project.md` (GH #381). Every queue drainability check (preflight's
`queue_drainable_head_count` and supervisor head, `session-check` and the Codex
Stop continuation gate, the supervisor idle watch and orphan drain, `plan`) and
the preflight Todo-backlink audit therefore resolve the effective preset map,
not the document's own `presets` alone. Such a head:

- is drainable without a backing backlog/icebox item, so an `agent:backlog`
  that lacks the name does not turn it into a stale mirror;
- owes no Todo backlink, so preflight does not emit
  `tracked_work_missing_todo_backlink` for it; and
- routes by its invocation text, never by the bare preset name.

A name that is also open tracked work (an open backlog, review, or icebox item)
keeps tracked-item precedence, exactly as for a document-local preset. The
inherited map is resolved on read for these checks and is never written into
the session document. Because drainability grants no authorization, an
unreadable or malformed project file degrades these checks to the document's
own presets; preset expansion keeps failing closed on the same file.

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
- a queue head naming a project-only preset is drainable with an
  `agent:backlog` that lacks the name and draws no
  `tracked_work_missing_todo_backlink` signal, while the same name as open
  backlog work stays tracked (GH #381);
- project presets interpolate document-local overrides recursively without
  rewriting either document; and
- missing explicit references, unset declared values, and cycles fail closed,
  while undeclared ordinary hashtags remain literal text.
