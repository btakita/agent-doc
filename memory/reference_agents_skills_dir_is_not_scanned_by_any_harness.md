---
name: reference_agents_skills_dir_is_not_scanned_by_any_harness
description: "Only Claude Code skips `.agents/skills/` — Codex, OpenCode, Grok DO scan it (corrected 2026-10-04); personal link needed for Claude only."
metadata: 
  node_type: memory
  type: reference
  originSessionId: 2d88e5a0-920a-4118-973b-e44bb53b6733
  modified: 2026-09-22T17:24:28.624Z
---

**CORRECTED 2026-10-04 (`#tsiftharnesslinks`, tsift `5ebe6e3`):** the original claim "no harness scans `.agents/skills/`" was WRONG for three of four harnesses. Verified against docs + installed binaries:

| harness | user skills dir | scans `~/.agents/skills`? |
|---|---|---|
| Claude Code | `~/.claude/skills/` | **No** — the only one that needs the personal link |
| Codex | `$CODEX_HOME/skills/` (`~/.codex/skills/`) | Yes (documented user scope) |
| OpenCode | `$OPENCODE_CONFIG_DIR` / `$XDG_CONFIG_HOME/opencode/skills` (`~/.config/opencode/skills`) | Yes (also `~/.claude/skills`); undocumented `~/.opencode` also read |
| Grok | `$GROK_HOME/skills/` (`~/.grok/skills/`) | Yes (also `~/.claude/skills`; `grok inspect --json` confirms) |

So a personal link for Codex/OpenCode/Grok only adds a duplicate entry (OpenCode warns on duplicate names). tsift ≤0.1.103 personal mode linked `~/.opencode/skills` — wrong target; fixed to the XDG path with stale-link cleanup.

**Test hazard found:** tsift test `off_mode_removes_the_harness_link_but_not_an_unmanaged_one` used the REAL `$HOME` and deleted `~/.claude/skills/tsift`; now uses a tempdir. If the Claude link vanishes, re-run `tsift init --instructions personal --harness claude`.

**Consequence for tsift (before 0.1.100+`#harnessskilllink`):** `tsift init --instructions personal` installed `~/.agents/skills/tsift/SKILL.md` and claimed it removed the need for `AGENTS.md`. It did not — the only thing that ever made the canonical skill reachable was the marker-owned `## Code Navigation` router that `shared` mode writes into `AGENTS.md`. Personal mode installed a skill **nothing loaded**.

**How this presents (verified 2026-09-22):** `~/.agents/skills/tsift/SKILL.md` sat at the current `tsift:skill v=0.1.100` while the skill Claude Code actually loaded was a hand-written `~/.claude/skills/tsift/SKILL.md` pinned at `tsift-version: 0.1.62` — 38 versions stale. `tsift status` reported instructions **current** the whole time, because status only ever reads the canonical path. A version check on the loaded skill and a version check on the canonical skill are different measurements; only the canonical one is what tsift reports.

**`tsift audit` used to miss it too, and now catches it (`#skillversioneval`, commit `b4bedd1`).** A stale skill has a `SKILL.md` and a `description`, which was everything the audit checked — it reported `healthy: 3, broken: 0` against the 0.1.62 file. It now compares the version the skill *declares* (`<!-- tsift:skill v=… -->` marker, else `tsift-version:` frontmatter; marker wins) against the running binary's `CARGO_PKG_VERSION`, and a mismatch is an issue that makes the skill `broken`. A skill declaring neither is never flagged. **So: `tsift audit` is the instrument for this, not `tsift status`** — status answers "is the canonical surface current", audit answers "is the surface the harness actually loads current".

**Fix:** `tsift init --harness <claude|codex|opencode|grok|all>` (commit `aac7c11`) symlinks the canonical skill into each harness dir — project-relative in `shared`, home-rooted absolute in `personal`. **Symlink, never copy:** a copy is exactly how the 0.1.62/0.1.100 split happened. It fails closed on any path that is not a tsift-owned link, so a hand-maintained `~/.claude/skills/tsift/` blocks `--harness claude` (and, because `--harness all` bails on the first refusal, blocks the whole fan-out) until it is moved aside.

Related: [[feedback_tsift_skill_primary_source]], [[feedback_agents_md_canonical]].
