---
agent_doc_session: 1ef5f0e7-0687-4f35-b9bc-2b9c330d5688
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---

## Status

<!-- agent:status patch=replace -->
agent-doc v0.31.24 — active
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
Sessions v0.31.18–v0.31.24. Key work:

1. v0.31.18-v0.31.19: cross-session swap-pane bug fix (rescue_from_stash session guard), slash command dispatch from diff, partial compact --keep N, JB 0.2.48 snapshot-timestamp dedup guard.

2. PyPI trusted publisher: add GitHub Actions publisher for btakita/agent-doc/.github/workflows/pypi.yml + env 'pypi'.

3. ❯ prefix normalization (v0.31.20): user-added exchange lines get ❯ prefix. Binary-side in write.rs. Both append+replace modes. IPC path: normalize_prefix_lines sent to plugin.

4. Overeager prefix bug (v0.31.21): was using content_ours user-region (includes agent response). Fix: diff snapshot→baseline to identify true user additions.

5. Release gate rule: after cargo install --path ., pause for user confirmation before publishing upstream.

6. Slash command routing: builtin_commands vs slash_commands separated in preflight JSON. Plugin autocomplete discovers skills from ~/.claude/skills/.

7. Git archive (v0.31.23): agent-doc log/show/diff subcommands; compact auto-tags pre-compact commit as agent-doc/<name>/pre-compact-N; --tag <name> flag.

8. fullContent + patches conflict (v0.31.23): fullContent overwrites response on next cycle. Fix: only send fullContent when no component patches present. JB + VSCode + binary all fixed.

9. Last-boundary normalization: use LAST boundary (not first) for user region detection. JB findAll().lastOrNull(), VSCode + write.rs updated.

10. normalize_prefix_lines HashSet bug: positional zip comparison replaces set-membership to avoid cross-position false-negatives.

11. Fence-interior ❯ bug (v0.31.23-v0.31.24): lines inside fenced regions were getting ❯. Fixed by tracking fence state in both diff+apply loops. ~~~ tilde fence gap also fixed in v0.31.24 via fence_open/fence_close helpers.

12. Plugin version bump rule: any plugin change requires version bump.
13. Commit-after-write rule: always call agent-doc commit after every agent-doc write.

Open: 2-pane layout loss when navigating to non-agent-doc file in editor (tmux collapses to 1 pane). Not yet fixed.

❯ Please fix https://github.com/btakita/agent-doc/actions/runs/24167784102
### Re: CI fix — collapsible_if in git.rs

Clippy error in `src/git.rs:410` — nested `if` that CI flags as `collapsible_if` on Rust 1.94.1:

```rust
// before
if !in_fence {
    if let Some(stripped) = line.strip_suffix(" (HEAD)") { ... }
}

// after
if !in_fence && let Some(stripped) = line.strip_suffix(" (HEAD)") { ... }
```

Fixed, all 743 tests pass. Committed as `7fc7905`, pushed — CI should be green.

❯ Do we need a new version or is this harmless for the current release?
### Re: new version needed?

No re-release needed. The published v0.31.24 binary (crates.io/PyPI/GitHub Release) was built from `1305ce9` — before `7fc7905` was introduced. The clippy issue only exists in main post-release, so published users are unaffected.

The fix just unblocks CI on main (prevents the red check on new PRs). Fold it into v0.31.25 whenever there's enough to release.

❯ When I ran `compact exchange` in session-share/tasks/claudescore.md, all of the summary was uncommitted. Why?
### Re: compact exchange — uncommitted summary (HEAD)

The compact summary **was** committed to git (`50f67f9`), but the skill used a raw `git commit` instead of `agent-doc commit`:

```
# what actually ran:
git commit -m "Compact exchange — ClaudeScore session history archived"

# what should run:
agent-doc commit src/session-share/tasks/claudescore.md
```

`agent-doc commit` writes a **VCS refresh signal** after committing so the editor updates its git gutter. A raw `git commit` skips that signal — the editor kept showing all ~32 summary lines as green (uncommitted) even though git had already committed them. The summary was in git; the editor just didn't refresh.

**Fix:** The compact-exchange runbook step 4 says `agent-doc commit <FILE>` — that's correct. The skill that ran the compact substituted a manual `git commit -m "..."` instead, which is the bug. `agent-doc commit` always uses the `agent-doc(<stem>): <timestamp>` format, so if a custom message were needed that's a missing feature — but for correctness and editor state, `agent-doc commit` must be used.
<!-- agent:boundary:2f66776e -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] Verify CI passes on 7fc7905
<!-- /agent:pending -->
