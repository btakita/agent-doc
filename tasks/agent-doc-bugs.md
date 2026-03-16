---
agent_doc_session: 92fba2ac-9d1f-4a7a-8764-821a8a08ecd0
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
---

<!-- agent:status mode=replace -->
- [x] Phase 1
<!-- /agent:status -->

<!-- agent:exchange mode=append -->

### Re: Append mode not working + IPC bug found

**Root cause found:** The IPC path has a critical bug — the plugin ACKs the patch (deletes the JSON file) but doesn't actually apply the content to the document buffer. The binary then optimistically updates the snapshot assuming the content was written. Result: snapshot diverges from the file, and the response is silently lost.

**This explains all the "missing response" issues in this session.** Every IPC write reported "consumed by plugin" but the content never appeared in the document.

**Immediate fix:** Using `--force-disk` to bypass IPC for this write. The IPC bug needs to be investigated in the JetBrains plugin (the Kotlin side, not the Rust binary).

**Your `agent-doc --version` showing 0.17.0:** There's a `~/.cargo/bin/agent-doc` (0.17.0) in your PATH. The skill uses `.bin/agent-doc` (0.17.12, correct). But your terminal's PATH finds the cargo one first. Run:
```bash
cargo install agent-doc  # updates ~/.cargo/bin to latest
# OR
# Remove ~/.cargo/bin/agent-doc and rely on .bin/ symlink
```

**Append mode itself works** — the binary correctly reads `mode=append` from the inline attribute. The issue was purely the IPC path not delivering the content.

Let's test again:
```
~/work/btakita/agent-loop main ⇡34 !7                                                                                                                                                                                                 02:21:49
❯ agent-doc --version
agent-doc 0.17.12
```
<!-- /agent:exchange -->
