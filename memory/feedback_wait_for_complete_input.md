---
name: Wait for complete user input
description: Don't act on partial/incomplete user input — wait for the full message before taking action
type: feedback
---

Never act on partial input (e.g., a single letter "S") as if it's a complete instruction. Wait for the user to finish typing their full request before taking action, especially for irreversible operations like sending emails.

**Why:** User typed "S" which was the start of "Save as a draft on gmail so I can verify", but the agent interpreted it as approval to send immediately. The email was sent before the user could finish their instruction.

**How to apply:** For any ambiguous or incomplete-looking input (single characters, sentence fragments), ask for clarification rather than guessing the intent. This is especially critical for send/delete/publish operations that can't be undone.
