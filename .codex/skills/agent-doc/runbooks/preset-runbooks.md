# Preset Runbooks

Use a preset-associated runbook when a reusable prompt needs a longer procedure
that should remain navigable without inflating every agent-doc turn.

## Declare an association

Keep short intent in `prompt` and route procedure detail through `runbook`:

```yaml
presets:
  '#release':
    prompt: release + publish
    runbook: runbooks/release.md
```

Scalar presets remain valid. `agent-doc runbook create <FILE> release --preset
'#release'` promotes the scalar to this structured form while preserving its
prompt body and unrelated frontmatter.

When guidance differs by harness, use an agent-aware structured preset. The
active agent receives its canonical override and every other agent receives the
`default` fallback:

```yaml
presets:
  '#release':
    default: release + publish
    codex: release + publish; report the worktree branch and commit
    claude: release + publish; include file-and-line citations
    runbook: runbooks/release.md
```

The canonical override keys are `claude`, `codex`, `opencode`, and `grok`.
Scalar presets continue to apply to every agent, and legacy `prompt:` remains a
valid fallback in structured entries.

Runbook paths are project-relative Markdown files under `runbooks/` or
`.agent-doc/runbooks/`. Absolute paths, `..`, missing files, paths outside those
directories, and symlink escapes are rejected.

## Navigate in a harness

- `agent-doc runbook list <FILE>` shows the deterministic project catalog,
  preset associations, descriptions, and exact expansion commands.
- Add `--json` for a machine-readable catalog.
- `agent-doc runbook show <FILE> <#preset|catalog-name|relative-path>` loads one
  catalogued runbook; add `--json` for metadata plus content.
- When an invoked preset has a runbook, preflight emits a `Required:` load
  instruction with its validated path. Load it before acting. Preflight does not
  inject runbook content unconditionally.

## Author safely

Run `agent-doc runbook create <FILE> <kebab-name> [--preset <#id>]
[--description <text>]`. The command creates `runbooks/<kebab-name>.md` with a
small procedure/verification scaffold, refuses collisions, and updates only an
existing preset association through the preserving frontmatter writer. Edit the
new runbook normally, keeping deterministic actions and verification evidence in
the committed file.
