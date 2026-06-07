<!-- tsift:opencode-command v=0.1.62 name=tsift-memory-guard -->
---
description: Guard a memory or tool payload before model handoff
---

Run `tsift memory budget-guard --file <target> --json` when `$ARGUMENTS` names a file, or `tsift memory budget-guard --text '<payload>' --json` for inline payload text. Summarize whether the payload is allowed, the estimated token count, replacement digest/context commands, and retryable chunk commands; file retry commands may include `--byte-start` / `--byte-end`. Do not send the raw payload to a model when the guard returns `blocked_split_required`.
