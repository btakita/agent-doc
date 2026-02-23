# agent-doc

> **Alpha software.** Expect breaking changes between minor versions. See the [changelog](reference/changelog.md) for migration notes.

agent-doc turns any markdown document into an interactive session with an AI agent. Edit the document, press a hotkey, and the tool diffs your changes, sends them to the agent, and writes the response back into the document. The document is the UI.

## Why documents?

Terminal prompts are ephemeral. You type, the agent responds, the context scrolls away. Documents are persistent — you can reorganize, delete noise, annotate inline, and curate the conversation as a living artifact. The agent sees your edits as diffs, so every change carries intent.

## How it works

1. **You edit** a markdown document with `## User` / `## Assistant` blocks
2. **You run** via hotkey or CLI — agent-doc computes a diff since the last run
3. **The agent responds** — the response is appended as a new `## Assistant` block
4. **Your editor reloads** — the document now contains the full conversation

The diff-based approach means you can also edit previous responses, delete noise, add inline annotations, and restructure the document freely. The agent sees exactly what changed.

## Features

- **Session continuity** — YAML frontmatter tracks session ID for multi-turn conversations
- **Merge-safe writes** — 3-way merge if you edit during an agent response
- **Git integration** — auto-commit for diff gutter visibility in your editor
- **Agent-agnostic** — Claude backend included, custom backends configurable
- **Editor integration** — JetBrains, VS Code, Vim/Neovim via hotkey

## Tech stack

- **Language**: Rust (2021 edition)
- **CLI**: `clap` (derive macros)
- **Diffing**: `similar` crate (pure Rust)
- **Serialization**: `serde` + `serde_yaml` / `serde_json` / `toml`
- **Hashing**: `sha2` for snapshot paths
