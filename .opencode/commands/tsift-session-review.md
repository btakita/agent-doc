<!-- tsift:opencode-command v=0.1.62 name=tsift-session-review -->
---
description: Summarize bounded agent session context
---

Run `tsift --envelope session-review <target> --next-context --budget normal`, where `<target>` is `$ARGUMENTS` or `.` when no argument is provided. Summarize prompt targets, unresolved failures, touched files/symbols, and next digest commands. Do not replay raw transcripts.
