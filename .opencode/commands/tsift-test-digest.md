<!-- tsift:opencode-command v=0.1.69 name=tsift-test-digest -->
---
description: Run tests through the bounded digest runner
---

Run a bounded test digest. If `$ARGUMENTS` names a test command, run `tsift --envelope digest-runner --kind test --path . --shell-command '<command>'`; otherwise choose the project test command from the local instructions and wrap it the same way. Summarize failing tests, failure lines, and artifact handles.
