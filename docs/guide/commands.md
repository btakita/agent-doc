# Commands

All commands are available through the `agent-doc` CLI.

## submit

```
agent-doc submit <FILE> [-b] [--agent NAME] [--model MODEL] [--dry-run] [--no-git]
```

Diff, send to agent, append response. The core command.

| Flag | Description |
|------|-------------|
| `-b` | Auto-create branch `agent-doc/<filename>` on first submit |
| `--agent NAME` | Override agent backend |
| `--model MODEL` | Override model |
| `--dry-run` | Preview diff and prompt size without sending |
| `--no-git` | Skip git operations (branch, commit) |

Flow:
1. Compute diff from snapshot
2. Build prompt (diff + full document)
3. Pre-commit user changes (unless `--no-git`)
4. Send to agent
5. Append response as `## Assistant` block
6. 3-way merge if file was edited during response
7. Save snapshot (no post-commit — agent response stays as uncommitted changes)

## init

```
agent-doc init <FILE> [TITLE] [--agent NAME]
```

Scaffold a new session document with YAML frontmatter and a `## User` block. Fails if the file already exists.

## diff

```
agent-doc diff <FILE>
```

Preview the unified diff that would be sent on the next submit. Useful for checking what changed before committing to a submit.

## reset

```
agent-doc reset <FILE>
```

Clear the session ID from frontmatter and delete the snapshot. The next submit starts a fresh session.

## clean

```
agent-doc clean <FILE>
```

Squash all `agent-doc:` git commits for the file into a single commit. Useful for cleaning up history after a long session.

## audit-docs

```
agent-doc audit-docs
```

Audit instruction files (CLAUDE.md, AGENTS.md, README.md, SKILL.md) against the codebase:
- Referenced file paths exist on disk
- Combined line budget under 1000 lines
- Staleness detection (docs older than source)
- Actionable content checks

## Global flags

```
agent-doc --version    # Print version
agent-doc --help       # Show help
```
