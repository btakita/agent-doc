---
agent_doc_session: fad704e4-e741-40a5-8842-9b33ab87c1d3
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
❯ I want to run agent-doc using the Cursor editor. Since Cursor is based on vscodium or vscode, the VSCode plugin should work? Do I need special logic or a separate plugin to support cursor? Should I have the 2 tmux pane layout for now? Is there any way to integrate with the Cursor AI subscription?
### Re: Cursor editor support — opus-4-6

**Yes, the VSCode plugin works on Cursor as-is.** Cursor is a VS Code fork that supports the same extension API — there's no compatibility layer or separate build needed. The single `.vsix` binary runs on both. Installation is already supported:

```bash
agent-doc plugin install cursor   # Cursor-specific install
agent-doc plugin install vscode   # VS Code install
```

No special logic or separate plugin required. The extension uses standard VS Code APIs (`WorkspaceEdit`, `FileSystemWatcher`, `onLanguage:markdown` activation) — nothing Cursor-incompatible.

**Tmux pane layout:** Yes, you still need the 2-pane layout for now. agent-doc routes documents to Claude Code CLI sessions running in tmux panes. The editor plugin handles submit/claim/sync but the agent loop itself runs in tmux. The architecture is:

- **Left pane:** Claude Code CLI (the agent)
- **Right pane:** Cursor (or any editor) with the `.md` document open
- Plugin bridges them via file-based IPC (`.agent-doc/patches/`) and FFI (`libagent_doc.so`)

**Cursor AI subscription:** No integration path currently. agent-doc is built around Claude Code CLI — the agent loop runs in tmux, not inside Cursor's AI features (Cursor Tab, Cmd+K, Composer). Cursor's AI subscription gives you their built-in copilot features, which are orthogonal to agent-doc sessions. You'd use both independently:

- Cursor's AI for inline completions and quick edits
- agent-doc for structured document sessions with Claude Code

A future integration could theoretically use Cursor's extension API to bridge their AI, but the architectures are fundamentally different — Cursor's AI is editor-embedded while agent-doc is CLI-first with editor as a thin presentation layer.
<!-- agent:boundary:f0b6877b -->
<!-- agent:boundary:97ac9572 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
<!-- /agent:pending -->
