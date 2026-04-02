---
agent_doc_session: bef800bc-950e-4f9e-b861-92c249853397
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---

## Status

<!-- agent:status patch=replace -->
Sync auto-start bug fixed. Unclaimed files no longer spawn sessions on tab navigation.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/4674cbdd322fce519fe66718682225f86b088adb9a6e63aa6f5df754c59a4ef1-20260402-030850.md`*

The JB editor is very laggy with typing. Please look in the logs.
### Re: Editor lag — Tailwind CSS LSP is the culprit

The IDEA log shows the **Tailwind CSS language service** sending `textDocument/didChange` for every single keystroke, with the **entire file content** (~180KB) in each message. Every character you type triggers a 180KB JSON payload to the Tailwind CSS LSP.

This is not an agent-doc issue — it's the Tailwind CSS IntelliJ plugin processing `.md` files it shouldn't care about.

**Fix options:**
1. **Disable Tailwind CSS for this project** (Settings → Languages & Frameworks → Tailwind CSS → uncheck Enable)
2. **Exclude .md files** from Tailwind CSS processing (if the plugin supports file type filtering)
3. **Disable the Tailwind CSS plugin entirely** (Settings → Plugins → search "Tailwind" → disable)

Option 1 is the least disruptive — agent-doc documents don't use Tailwind CSS.
Option 1
### Re: Tailwind CSS disabled (HEAD)

Go to Settings → Languages & Frameworks → Tailwind CSS → uncheck "Enable Tailwind CSS support for this project". This stops the LSP from processing every keystroke in `.md` files.

If the lag persists after unchecking, restart the IDE to fully stop the Tailwind CSS language server process.
<!-- agent:boundary:b9c42c56 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [x] All implementation, tests, audit docs, and release complete
- [x] Published: crates.io ✓, GitHub Release ✓
- [x] CI fixes: skill unit tests + CLI integration tests + Windows compilation
- [x] Disable Tailwind CSS LSP for agent-loop project
- [ ] Evaluate utility of `/btw` (harness preemption) for agent-doc sync
<!-- /agent:pending -->
