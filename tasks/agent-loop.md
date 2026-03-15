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

<!-- agent:exchange -->

Yes — the .iml files were committed to 6 submodules. Fixed:

1. **Reset** each .iml-only commit
2. **Added `*.iml` to `.gitignore`** in each submodule
3. **Untracked** the .iml files (kept on disk)
4. **Force-pushed** to replace the bad commits

| Submodule | New Commit |
|-----------|------------|
| philosophy | `3a22f35` |
| resume | `60db19f` |
| censible-core | `bcf3f65cd` |
| cloudflare | `f9012d2` |
| cogov-dev | `1d2c8c0` |
| herbaliciousbliss-dev | `2d2bdc9` |

Parent pointers updated → `fdb95ce`, pushed to `main`.

Please commit & push submodules..commit and push agent-loop
<!-- /agent:exchange -->
