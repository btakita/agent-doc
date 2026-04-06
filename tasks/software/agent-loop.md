---
agent_doc_session: be20dd6e-31ce-4f16-93ed-42c9c2a21b98
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
agent_doc_auto_compact: 250
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
**Decisions/outcomes:**
- Auto-compact: user wants more control; aggressive auto-compact unwanted. Partial auto-compact (keep active topics, archive older) preferred but not yet implemented. `agent_doc_auto_compact: 250` set as safety net.
- Eval tuning (all done): system prompt added explaining do/conciseness/### Re: header conventions; token budgets tuned per case; `simple-question-concise` fixed (0.3→0.7) by clarifying headers are multi-topic only.
- Partial compact feature was considered but dropped from pending by user.

Setup pyproject.toml
I want to have vllm as a dependency.
My goal is to run Qwen3-Coder-Next (80B) on vllm to avoid running out of claude tokens over the next couple of days
### Re: Setup pyproject.toml with vllm (HEAD)

`pyproject.toml` was empty — populated it with `vllm` as a dependency:

```toml
[project]
name = "agent-loop"
version = "0.1.0"
requires-python = ">=3.10,<3.13"
dependencies = [
    "vllm",
]
```

**Python version issue:** vllm requires Python 3.9–3.12. The project's `.mise.toml` pins Python 3.14 (too new). You have 3.12.12 available via mise.

To use vllm, either:
1. Override Python for this project: `mise use python@3.12` (updates `.mise.toml`)
2. Or create a separate venv with 3.12: `mise exec python@3.12 -- uv venv .venv && source .venv/bin/activate && uv pip install vllm`

For Qwen3-Coder-Next 80B you'll also need enough VRAM — 80B at fp16 is ~160GB, so either 2× H100 or use quantization (e.g. AWQ/GPTQ ~40–80GB). Run with:
```bash
vllm serve Qwen/Qwen3-Coder-Next-80B --tensor-parallel-size 2
```
<!-- agent:boundary:efc28728 -->
<!-- /agent:exchange -->

<!-- agent:pending patch=replace -->
- [ ] Fix Python version for vllm: run `mise use python@3.12` in agent-loop root
- [ ] Install vllm: `uv pip install vllm` in a 3.12 venv
- [ ] Serve Qwen3-Coder-Next-80B via vllm
<!-- /agent:pending -->
