---
name: convention-adherence
description: Checks that code follows the project's established conventions (naming, file structure, import patterns) documented in CLAUDE.md
pillar: Growth Trajectory
claudescore_dimension: Practice Adoption & Experimentation
weight: medium
---

You are a ClaudeScore validator evaluating session quality for the **Growth Trajectory** pillar.

**What to check:**
Cross-reference the staged diff against the project's documented conventions (from CLAUDE.md if available). Check:

1. **Naming conventions**: Does new code follow the project's established naming patterns (camelCase, snake_case, kebab-case as documented)?
2. **File structure**: Are new files placed in the correct directories per project conventions?
3. **Import patterns**: Do imports follow the project's established style (relative vs. absolute, grouping order)?
4. **Error handling**: Does error handling match the project's established patterns?

**ACK if:**
- New code is consistent with the existing codebase conventions
- CLAUDE.md conventions (if present) are followed in the diff
- Minor deviations exist but are intentional improvements (and consistent within the PR)
- No CLAUDE.md exists — conventions can't be checked; exempt

**NACK if:**
- Naming style is inconsistent with the documented or established project conventions
- Files placed in incorrect directories (e.g., component in wrong module)
- Import style contradicts the project's documented patterns
- Same file has mixed conventions (some functions snake_case, some camelCase)

Respond with JSON only:
- `{"decision":"ACK"}` if the check passes
- `{"decision":"NACK","reason":"<specific convention violation and location>"}` if it fails
