---
agent_doc_session: 92fba2ac-9d1f-4a7a-8764-821a8a08ecd0
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
---

<!-- agent:exchange -->

### Re: Prompted fallback + merge strategy

**Prompted option after 5s:** This can't happen in the binary alone — `agent-doc write` is a one-shot CLI process (pipe stdin, write, exit). The prompt would need to come from the **skill** (Claude session). The flow:

1. `agent-doc write --stream` tries IPC, times out after 2s
2. Returns exit code indicating "IPC pending, no disk write"
3. The skill detects this exit code and asks the user: "IPC timeout — write to disk anyway? This will overwrite any unsaved editor changes."
4. If user says yes: skill re-runs with `--force-disk`
5. If user says no: response stays in patch file

This is cleaner than building interactive prompts into the binary. The skill is the interactive layer.

**3-way merge vs CRDT for the fallback write:**

The current `--stream` flag already does CRDT merge (baseline vs current file vs agent response). When falling back to disk write, it uses the same CRDT path — so user edits made between baseline save and write are preserved via CRDT merge. The disk fallback isn't the problem; the problem is that the CRDT merge reads the *disk file* (which may be stale relative to the editor buffer).

The IPC path avoids this entirely because the plugin applies patches to the live Document API (in-memory editor buffer). That's why IPC is strictly better when available.

**Summary:** The fix is skill-level (detect IPC timeout exit code, prompt user) + binary-level (new exit code for "IPC pending"). Want me to implement both?

Yes

I also want t
<!-- /agent:exchange -->
