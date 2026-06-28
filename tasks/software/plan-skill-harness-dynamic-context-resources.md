# Plan: skill-harness dynamic context resources

**Goal:** make skill-harness enforce the resource-router pattern for skills:
`SKILL.md` carries trigger and routing instructions, while bulky or mutable
context lives in `runbooks/`, `references/`, `scripts/`, `assets/`, or generated
context commands.

## Boundary

skill-harness owns packaging and validation:

- installing and checking full skill directories
- validating referenced resource files exist
- warning when `SKILL.md` appears to duplicate companion resources
- documenting dynamic context conventions for portable skills

skill-harness does **not** own runtime context caching. tsift owns reusable
context packs; agent-doc or the active harness owns per-session injection policy.

## Phases

### Phase 1 - `#shctxrefs`: resource-reference validation

Extend validation/check-dir to scan `SKILL.md` for local links to `runbooks/`,
`references/`, `scripts/`, `assets/`, and `SPEC.md`, then fail or warn when
referenced resources are missing from the source directory.

Tests:
- missing linked runbook is reported
- existing linked reference passes
- external/http links are ignored by local resource validation

### Phase 2 - `#shctxinstall`: directory install as default for resource skills

Improve CLI output and skill instructions so `install-dir` is the default path
when a skill contains companion resources. Keep single-file `install` for
self-contained skills, but warn when the source `SKILL.md` references local
resources that cannot be copied by single-file install.

Tests:
- `install --file` warns on local resource links
- `install-dir` copies the linked resource tree
- `check-dir` reports missing/changed/extra resource files

### Phase 3 - `#shctxlint`: duplication and router lint

Add a conservative lint that flags likely context rot:

- long `SKILL.md` sections that duplicate linked runbook headings
- copied code blocks that also appear in `scripts/`
- repeated schema-like tables that also appear in `references/`

This should start as warning-only. The goal is to steer authors toward one
canonical owner, not block legitimate concise examples.

Tests:
- duplicated runbook heading/body snippet emits a warning
- short routing sentence does not warn
- intentionally self-contained single-file skill stays valid

### Phase 4 - `#shctxschema`: dynamic context metadata

Define optional metadata that lets skills declare generated context commands
without making skill-harness run them:

```yaml
dynamic_context:
  - name: code-context
    command: tsift --envelope context-pack {query} --budget normal
    cache_owner: tsift
```

Validate shape and document that runtime harnesses decide whether to execute the
command. Do not make this required for ordinary skills.

Tests:
- valid metadata parses
- invalid command shape reports a deterministic error
- installs preserve metadata unchanged across Claude/Codex/OpenCode/generic

## Acceptance

- Full skill directories remain the portable source of truth.
- Authors get actionable warnings before static/dynamic context drifts.
- skill-harness stays a packaging/validation tool and does not grow a runtime
  context database.
- `cargo test --manifest-path src/skill-harness/Cargo.toml compose` and the
  install/check matrix pass.

