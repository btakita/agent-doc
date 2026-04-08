# Cleanup Runbook

## When to run

- Manually: `agent-doc gc [--dry-run] [--root <path>]`
- Automatically: preflight runs GC once per day (via `.agent-doc/gc.stamp`)

## What `agent-doc gc` cleans

| Directory | What it contains | Cleanup rule |
|-----------|-----------------|--------------|
| `snapshots/` | Document snapshots (`<hash>.md`) | Remove if no matching document exists |
| `crdt/` | CRDT state (`<hash>.yrs`) | Remove if no matching document exists |
| `crdt/` | CRDT locks (`<hash>.yrs.lock`) | Remove if >1 hour old (stale) |
| `pre-response/` | Pre-write backups (`<hash>.md`) | Remove if no matching document exists |
| `baselines/` | Merge baselines (`<hash>.md`) | Remove if no matching document exists |
| `locks/` | Snapshot locks (`<hash>.md.lock`) | Remove if >1 hour old (stale) |
| `hooks/post_write/` | Hook event files | Remove if >24 hours old |
| `hooks/post_commit/` | Hook event files | Remove if >24 hours old |

## Safe manual cleanup

If `agent-doc gc` is unavailable, these are safe to run manually:

```bash
# Preview what would be deleted
agent-doc gc --dry-run

# Remove stale locks (>1 hour old)
find .agent-doc/locks -name "*.lock" -mmin +60 -delete
find .agent-doc/crdt -name "*.lock" -mmin +60 -delete

# Remove old hook events (>24 hours old)
find .agent-doc/hooks -name "*.json" -mmin +1440 -delete
```

## How files accumulate

- **Orphaned snapshots**: documents are deleted but their snapshot/crdt/pre-response files remain (keyed by path hash)
- **Stale locks**: process crashes leave lock files that are never cleaned up
- **Hook events**: `poll()` cleans expired events, but `gc()` requires manual invocation
- **Pre-response files**: before v0.31.5, successful writes did not delete pre-response backups

## Adding new `.agent-doc/` directories

When adding a new hash-keyed directory to `.agent-doc/`:
1. Add cleanup logic to `gc.rs` (in the directory scan loop)
2. Ensure the directory is documented in this runbook
3. If files are temporary (like locks), ensure the Drop impl or caller deletes them
4. If files are per-document (like snapshots), ensure `rename.rs` migrates them on document rename
