# Investigate agent-doc Behavior

When debugging unexpected agent-doc behavior, follow these steps systematically.

## 1. Check agent-doc logs

**ops.log** — high-level operational events (writes, commits, snapshots):
```bash
grep "<filename>" .agent-doc/logs/ops.log | tail -30
```

Key fields to look for:
- `write_origin` — who triggered the write (`skill`, `stream`, `cli`)
- `ipc_write_attempt` — `patches=N` (matched), `unmatched_len=N` (dropped content)
- `commit_staging` — `snap_len` vs `file_len` (drift = unexpected changes)
- `drift_warning` — file diverged from snapshot

**debug.log** — verbose per-day logs:
```bash
grep "<filename>" .agent-doc/logs/debug.log.$(date +%Y-%m-%d) | tail -30
```

## 2. Check git history

```bash
# All commits touching the file
git log --oneline -- <file>

# Commits in the time window
git log --oneline --after="<start>" --before="<end>"

# What was committed
git show <commit>:<file>

# Diff between commits
git diff <commit1>..<commit2> -- <file>

# Who added the file
git log --diff-filter=A -- <file>
```

## 3. Check affected files

- Read the current file content
- Check the snapshot: `.agent-doc/snapshots/<sha256(path)>.md`
- Compare file vs snapshot sizes (drift indicates out-of-band writes)
- Check frontmatter: is the format correct (`template` vs `inline`)?
- Check components: are `<!-- agent:name -->` tags present?

## 4. Common failure patterns

| Symptom | Likely cause | Where to look |
|---------|-------------|---------------|
| Content silently dropped | `patches=0 unmatched_len=N` in ops.log — patches didn't match any component | File format (append vs template), missing components |
| File created but empty | `agent-doc write` auto-created in append mode, template patches didn't match | ops.log `write_origin` + `ipc_write_attempt` |
| Wrong file format | File was auto-created by `write` instead of `init` | Frontmatter: missing `agent_doc_format: template` |
| Snapshot drift | Out-of-band edit or concurrent write | ops.log `drift_warning`, `commit_staging` snap/file mismatch |
| Transfer lost content | Target file lacked the component being transferred to | `extract.rs` fallback: appends to EOF without component tags |

## 5. Report template

When reporting findings, include:
- **Timeline:** ops.log entries with timestamps
- **Root cause:** which code path produced the bad outcome
- **Evidence:** git commits, file content, log entries
- **Fix:** what should have happened / what to change
