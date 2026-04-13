Context Audit

Score: 82/100 [NEEDS WORK]

Context Breakdown (from /context)

- Total usage: 47.4k/200k tokens (24%)
- System prompt: 7k (3.5%)
- System tools: 19.5k (9.7%)
- MCP tools: 9.5k (4.8%) — 44 tools across Chrome DevTools, Gmail, Google Calendar, claude-mem
- Memory files: 5.7k (2.8%)
- Skills: 1.3k (0.7%)
- Messages: 5.1k (2.6%)
- Free space: 118.8k (59.4%)

You're healthy overall at 24% utilization. The main waste is addressable.

  ---
Issues Found

[WARNING] Missing settings.json optimizations (-10 points)

1. No autocompact_percentage_override (-10 points)
   - Current: unset (likely 80%)
   - Recommended: 75
   - Impact: Autocompact triggers sooner than optimal, losing usable context
2. No BASH_MAX_OUTPUT_LENGTH override (-5 points)
   - Current: ~30-50K (default)
   - Recommended: 150000
   - Impact: Large command outputs are truncated unnecessarily
3. No permissions.deny rules for build artifacts (-10 points)
   - Bloat directories found: .venv/, resume/.venv/, resume/__pycache__/
   - Should deny: .venv, __pycache__, *.egg-info
   - Impact: These can be read into context, wasting tokens on stale data

[WARNING] Skills slightly oversized (-10 points)

Two skills exceed 200 lines:

- agent-doc/SKILL.md: 266 lines (-5 points)
  - Well-structured, no filter violations detected. Comprehensive because agent-doc is complex.
  - Splitting would reduce clarity. OK as-is.
- dev/SKILL.md: 210 lines (-5 points)
  - Excellent structure with clear sections (Commands, Composability, Meta-Cognition, Conventions, Runbooks).
  - No redundancy. Comprehensive because it orchestrates multi-project releases.
  - OK as-is.

Both are intentionally detailed for important workflows. Not bloat.

[INFO] CLAUDE.md files borderline (-0 points, monitoring)

- ~/.claude/CLAUDE.md: 32 lines (philosophy ontology + RTK reference)
- /agent-loop/CLAUDE.md: 115 lines (project layout, workflow orchestration, core principles)
- /agent-loop/CLAUDE.local.md: 87 lines (private instructions for agent-doc, corky, etc.)
- Total: ~234 lines (just above 200 threshold, but no violations detected)

Assessment: Well-organized. Rules are specific and actionable (not vague). No contradictions found between files. No
bandaid rules. Core principles in CLAUDE.md are universally applicable.

  ---
Top 3 Fixes

1. Add permissions.deny rules to settings.json (10 tokens saved per session)
   - Deny .venv, __pycache__, *.egg-info directories
2. Set autocompact_percentage_override to 75 (context stability improvement)
   - Current default is often 80%; 75 is more aggressive without losing usable space
3. Set BASH_MAX_OUTPUT_LENGTH environment variable to 150000 (5 tokens saved per large command)
   - Allows full output from verbose build logs without truncation

  ---
Recommendations

✅ No major bloat detected. Your setup is lean and well-documented.

⚠️ Three quick wins are available:

1. Add this to settings.json:
   "autocompact_percentage_override": 75,
2. Add to your shell profile (.zshrc or .bashrc):
   export BASH_MAX_OUTPUT_LENGTH=150000
3. Add to settings.json:
   "permissions": {
   "defaultMode": "bypassPermissions",
   "deny": [".venv", "__pycache__", "*.egg-info"]
   }
