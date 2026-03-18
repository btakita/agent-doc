---
agent_doc_session: be20dd6e-31ce-4f16-93ed-42c9c2a21b98
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
---

# agent-loop operations

Operational task document for agent-loop workspace management.

**History** (archived in git, commits up to `f80f524`):
- Submodule conversions (briantakita.me-dev, ctx-core-dev, mail, python-intentions)
- mail/mailboxes/lucas nested submodule init + push
- JetBrains module creation (20 .iml files)
- IDEA Cargo project path fixes (workspace.xml)
- Recursive `.agent-doc/` gitignore additions (20+ submodules)
- `.sync-state.json` gitignore for corky/mail
- Nested submodule fixes (ctx-core-dev/lib/relysjs, rappstack-dev/lib/ui--server--blog)

<!-- agent:exchange patch=append -->

### Re: Composable dev skill

Yes — implemented a CLAUDE.md-style configuration hierarchy for the dev skill. Updated both SKILL.md and SPEC.md.

**Resolution chain (deepest wins):**
```
SPEC.md (invariant — behavioral contract)
  ↓
SKILL.md (workspace defaults)
  ↓
src/<project>/DEV.md (project overrides — checked in)
  ↓
src/<project>/DEV.local.md (private overrides — gitignored)
```

**Per-project DEV.md** uses YAML frontmatter for structured overrides:
- `project_type` — override auto-detection
- `publication_targets` — custom list
- `extra_checks` / `skip_checks` — add or suppress verification steps
- `secret_paths` — per-target secret manager paths
- `pre_release` / `post_release` — custom hooks
- Free-form markdown below for human context

**Merge semantics** vary by field type:
- **Last wins:** `project_type`, `publication_targets`
- **Append:** `extra_checks`, `skip_checks`, `pre_release`, `post_release` (accumulate across layers)
- **Deep merge:** `secret_paths` (per-target, deepest wins)

**Design parallel:**
| Claude Code | Dev Skill |
|------------|-----------|
| `CLAUDE.md` (root) | `SKILL.md` (workspace) |
| `CLAUDE.md` (subdir) | `DEV.md` (per-project) |
| `CLAUDE.local.md` | `DEV.local.md` |
| `AGENTS.overrides.md` | Future: `DEV.overrides.md` |

**Backward compatible** — projects without DEV.md files fall back to auto-detection + SKILL.md defaults + CLAUDE.md. No migration needed.
<!-- /agent:exchange -->

<!-- agent:input -->
I want the process to create a SPEC to be highly configurable. The human should drive the evolution of the dev SPEC with AI assistance. We should encourage people to "borrow" from other skill repos.
<!-- /agent:input -->

<!-- agent:pending patch=replace -->
- [x] Make dev skill composable (CLAUDE.md-style hierarchy with subdirectory overrides)
- [ ] Respond to truncated message: "I want " (awaiting completion)
<!-- /agent:pending -->
