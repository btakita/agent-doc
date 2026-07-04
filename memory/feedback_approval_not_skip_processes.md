---
name: approval-not-skip-processes
description: "do"/"go"/"yes" means execute with full quality — NOT move fast and skip steps
type: feedback
---

Approval signals ("do", "go", "yes", "continue") mean "execute the approved work with full quality processes." They do NOT mean "move fast and skip verification steps."

**Why:** During 2026-04-04 session, rapid "do" chains caused speed-over-rigor bias. Tests were skipped, precommit runbook step 2 was treated as optional, and edge cases were only added reactively when the user asked. The user corrected: "I'm counting on you to be thorough with your processes."

**How to apply:** When receiving an approval signal:
1. Execute the approved work
2. Follow ALL applicable runbooks (precommit, verification, etc.)
3. Add tests as part of implementation, not as a separate step
4. Run `make check` before marking done
5. Never interpret "do" as permission to cut corners
