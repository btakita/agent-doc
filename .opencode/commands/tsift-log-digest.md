<!-- tsift:opencode-command v=0.1.69 name=tsift-log-digest -->
---
description: Run a verbose command through the bounded log digest
---

Run a bounded log digest. If `$ARGUMENTS` names a build, install, or verification command, run `tsift --envelope digest-runner --kind log --path . --shell-command '<command>'`; otherwise ask for the command before running. Summarize compact output, failures, and artifact handles.
