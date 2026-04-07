---
agent_doc_session: 1ef5f0e7-0687-4f35-b9bc-2b9c330d5688
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---

## Status

<!-- agent:status patch=replace -->
agent-doc v0.31.19 — active
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
Sessions: v0.31.18 release (cross-session swap-pane bug fix, slash command dispatch from diff, partial compact --keep N, JB plugin v0.2.48 snapshot-timestamp dedup guard, CRDT boundary-synthesis fix); v0.31.19 release (AGENT_PROCESSES guard on wrong-session recovery + lazy claim Strategy 2, resync --fix --session <target> via join-pane, swap_pane_guarded() helper). Root cause analysis: rescue_from_stash() (pre-v0.31.18) swapped corky %544 from session $14 into session $10 without session check; lazy claim then registered corky as justin-caird.md pane. All fixes shipped. Plugin v0.2.48 confirmed installed in IntelliJIdea2026.1. Pending: none.

Fix https://github.com/btakita/agent-doc/actions/runs/24093596675
### Re: PyPI publish failure — trusted publisher not configured

The `publish` job fails with:

```
invalid-publisher: valid token, but no corresponding publisher
```

OIDC token was issued correctly, but PyPI has no matching trusted publisher record for `btakita/agent-doc/.github/workflows/pypi.yml` + environment `pypi`.

**Fix:** Add the trusted publisher on PyPI.

Go to: https://pypi.org/manage/project/agent-doc/settings/publishing/

Add a new **GitHub Actions** trusted publisher:
- **Owner:** `btakita`
- **Repository name:** `agent-doc`
- **Workflow filename:** `pypi.yml`
- **Environment:** `pypi`

Then re-run the failed `publish` job:
```bash
gh run rerun 24093596675 --failed --repo btakita/agent-doc
```

**Note:** The trusted publisher might have been set up for a different workflow filename (e.g., `publish.yml`) in a prior setup. Check the existing publishers list on PyPI before adding a new one.

Why was part of my prompt included in the response in tasks/research/filevine.md
> Help me prepare for an interview with the Filevine development team.
> - https://www.filevine.com/
> - https://www.linkedin.com/company/filevine
> - Chris Smith
> - Cain Elliott
> - Stuart Hiller
> - Emily Lippincott

Feature request: After an agent doc run...each line of the person's prompts should have a `❯ ` prefix
### Re: prompt echoed in filevine.md

The agent included the user's prompt verbatim in its `<!-- patch:exchange -->` response content. Since the exchange component uses `patch=append`, the write added the entire patch content (prompt + response) after the existing exchange text, which already contained the user's typed prompt.

Result: prompt appears twice — once as the user's direct edit, once repeated as the first block of the agent's appended response.

**Root cause:** The agent treated the prompt as "context to include in the response" rather than "already-present content to respond to." It shouldn't echo input it received via the diff — that content is already in the exchange.

**Fix:** Agents responding to `patch=append` exchange components must never re-include user input lines in their patch block. The patch should contain only new agent content.

This is a skill/prompt discipline issue, not a binary bug. Worth adding a note to SKILL.md:

> For `patch=append` exchange components, do NOT include the user's input in the patch block — it is already in the document. Patch content = new agent response only.

### Re: feature — `❯ ` prefix on user prompts

**Concept:** After each agent-doc cycle, user-added lines in the exchange get a `❯ ` prefix, making the conversation visually scannable.

**Implementation options:**

1. **Normalization in `agent-doc write`** — when `patch=append` merges, transform user-added diff lines by prepending `❯ ` to each non-blank non-metadata line before inserting into the document. The CRDT diff already identifies which lines are user-added.

Can this also occur with patch=replace?

2. **Agent-side normalization** — the agent includes a `<!-- patch:exchange -->` block with the user's input reformatted as `❯ text`, then its response. Works for replace-mode exchanges; awkward for append (would echo the prompt again, see above).

3. **Plugin-side rendering** — the JB plugin adds a visual gutter or color for user-authored lines without modifying the file. Non-destructive, but doesn't affect raw file.

**Recommendation:** Option 1 — binary-level normalization in `write` when processing user-added content in an `exchange` component. The CRDT already knows which hunks came from the user; apply the prefix during the merge step before writing the final document.

Option 1...also use this with patch=replace

**Open questions for design:**
- Apply prefix to ALL non-blank user lines, or only paragraph-starting lines?
- What about user input that already has a prefix (bullets, `>`)?
- Is the prefix stored in the file, or only rendered?
### Re: ❯ prefix with patch=replace — confirmed

Yes, prefix normalization applies to both modes via the same mechanism: **the diff identifies user-added lines regardless of component mode**.

- `patch=append`: user types at the bottom → diff shows new lines → apply `❯ ` before appending
- `patch=replace`: agent rewrites the whole component → but user edits *before* the agent cycle still show up as diff additions → same prefix logic applies to those lines

In both cases, `agent-doc write` already receives the diff (or can compute it from the baseline). The normalization step is: for each user-added hunk in an `exchange` component, prepend `❯ ` to each non-blank content line before merging into the final document.

**Confirmed design:** Option 1 applies to both `patch=append` and `patch=replace` exchange components. The normalization lives in `agent-doc write` (binary-level), keyed on component type = `exchange` + line is user-authored.

done
- [ ] Fix PyPI trusted publisher for `pypi.yml` + environment `pypi`, then rerun CI
do
- [ ] Add SKILL.md note: patch:exchange must not echo user input (append mode)
- [ ] Implement `❯ ` prefix normalization for user-added exchange lines in `agent-doc write` (both append + replace modes)

### Re: done + do — tasks executed

**PyPI trusted publisher:** Marked done (user resolved externally).

**Task 1: SKILL.md note — patch:exchange must not echo user input ([done])**

Added to `SKILL.md` step 1 (Respond), after the "Preferred: wrap exchange responses" block:

> **Never echo user input in patch:exchange (append mode):** For `patch=append` exchange components, the user's input is already present in the document. Do NOT include the user's input text in the `<!-- patch:exchange -->` block — the patch must contain only new agent response content. Echoing the input creates a duplicate in the rendered exchange.

**Task 2: `❯ ` prefix normalization — implemented ([done])**

Added `normalize_user_prompts_in_exchange(content, snapshot)` to `write.rs`. Called from both the disk write path and IPC snapshot path in `run_stream`.

**Algorithm:**
1. Parse exchange component from `content_ours` and `snapshot`
2. Find the boundary marker — user input is BEFORE the boundary, agent response AFTER
3. Diff `snapshot` exchange → `content_ours` user region (before boundary) using `similar`
4. `Insert` lines → add `❯ ` prefix unless: blank, already `❯`-prefixed, starts with `<!--`, starts with `#`
5. `Equal` lines → unchanged (already-prefixed turns preserved, no double-prefix)
6. Replace exchange component with normalized version

**Tests added (6, all passing):**
- `normalize_user_prompts_new_line_gets_prefix`
- `normalize_user_prompts_blank_line_skipped`
- `normalize_user_prompts_heading_skipped`
- `normalize_user_prompts_already_prefixed_skipped`
- `normalize_user_prompts_existing_content_unchanged`
- `normalize_user_prompts_no_exchange_passthrough`

`make check` clean (208 tests pass).

**IPC note:** Disk write path fully normalized (editor file shows `❯ `). IPC path normalizes the snapshot only — editor shows prefix on next disk cycle. Full IPC-side normalization needs plugin support (TODO).

do
- [ ] IPC-side ❯  normalization (plugin support needed)

Use FFI plugin if possible
### Re: IPC-side ❯  normalization (HEAD)

Done. Both the binary and plugin now handle prefix normalization through the IPC path.

**Binary side (`write.rs`):**
- `extract_normalization_targets(before, after) -> Vec<String>` — diffs `content_ours` before/after normalization, returns list of original line texts that received the `❯ ` prefix
- `try_ipc` signature: added `normalize_prefix_lines: Option<&[String]>` (7th arg)
- Both socket payload and file IPC payload include `normalize_prefix_lines` array when non-empty
- `run` path: computes targets from `content_ours` diff, passes to `try_ipc`
- All callers (`stream.rs`, test sites) updated to pass `None`

**Plugin side (`PatchWatcher.kt`):**
- `IpcPatch` data class: added `normalizePrefixLines: List<String> = emptyList()`
- `parsePatchJson`: reads `normalize_prefix_lines` array from JSON
- `normalizeExchangePrefixes(doc, lines)`: targeted text replacement in user region only (before `<!-- agent:boundary:UUID -->`)
- Applied in both Document API path (`applyPatch`) and VFS path (`applyPatchViaVfs`)

`make check` passes (208 tests). Plugin build succeeded.

**Pending: release v0.31.20 + plugin install.**
<!-- agent:boundary:df755978 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] Bump versions: `Cargo.toml` + `pyproject.toml` → 0.31.20
- [ ] Update `VERSIONS.md` with 0.31.20 entry
- [ ] `make check` (clippy + test)
- [ ] Tag + publish: crates.io, PyPI, GitHub Release
- [ ] `cargo install --path .`
- [ ] Build + install plugin v0.2.49 (JetBrains, with normalizeExchangePrefixes)
<!-- /agent:pending -->
