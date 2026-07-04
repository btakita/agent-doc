---
name: Redact nested JSON values properly
description: When inspecting credential files, ensure redaction handles nested objects — not just top-level keys
type: feedback
---

When reading credential/auth files to inspect structure, NEVER print the file content even with a redaction script. Nested JSON structures bypass simple key-name redaction. Instead, only print the key names and types (e.g., `jq 'keys'` or `jq 'map_values(type)'`), never values.

**Why:** User livestreams. A naive Python redaction script checked top-level keys for 'token'/'key'/'secret' but the actual tokens were nested inside a `claudeAiOauth` object, so they printed in full.

**How to apply:** For ANY file that might contain secrets, use `jq '[paths] | map(join("."))' < file` to list key paths, or `jq 'map_values(type)' < file` for structure. Never pipe raw content through a "redaction" filter — it's too easy to miss nested values.
