# Quick Start

## Create a session document

```sh
agent-doc init session.md "My Topic"
```

This creates a markdown file with YAML frontmatter and a `## User` block ready for your first message.

## Write your first message

Open `session.md` in your editor and write under `## User`:

```markdown
---
session: null
agent: null
model: null
branch: null
---

# Session: My Topic

## User

Explain how TCP three-way handshake works.
```

## Submit to the agent

```sh
agent-doc submit session.md
```

The tool computes a diff, sends it to the agent, and appends the response as a `## Assistant` block. Your editor reloads the file with the response.

## Continue the conversation

Add a new `## User` block below the assistant's response, write your follow-up, and submit again:

```sh
agent-doc submit session.md
```

## Preview before submitting

```sh
agent-doc diff session.md       # see what changed since last submit
agent-doc submit session.md --dry-run  # preview the prompt without sending
```

## Basic workflow

```sh
agent-doc init session.md "Topic"   # scaffold a session doc
# edit session.md in your editor
agent-doc submit session.md         # diff, send, append response
# edit again, add follow-up
agent-doc submit session.md         # next turn
agent-doc clean session.md          # squash git history when done
```
