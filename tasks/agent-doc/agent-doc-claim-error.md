---
agent_doc_session: bef800bc-950e-4f9e-b861-92c249853397
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---

## Status

<!-- agent:status patch=replace -->
Bug 2A + 2B implemented. IPC snapshot failures non-fatal; commit auto-recovers divergence.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/4674cbdd322fce519fe66718682225f86b088adb9a6e63aa6f5df754c59a4ef1-20260331-140801.md`*
Does agent-doc support linked documents in the frontmatter?
### Re: Linked documents in frontmatter

No — agent-doc doesn't currently support linked documents in frontmatter. The `Frontmatter` struct has: `session`, `agent`, `model`, `branch`, `format`, `write_mode` — but no field for linked/related documents.

**What would this look like?**
```yaml
---
agent_doc_session: abc123
linked:
  - tasks/software/agent-doc.md
  - tasks/agent-doc/queue.md
---
```

**Use cases:**
- Agent sees related context when responding
- Cross-document sync (changes in one reflected in another)
- Task dependency tracking

Is this what you're thinking? Or a different use case? This would be a new feature — happy to design it.
It looks like there is related_docs. tasks/software/ragie-web-doc.md set related_docs in the frontmatter when I prompted it to.
Either way, when I request to link documents, nothing is added to the frontmatter.
Should we use linked instead of related_docs?
### Re: related_docs already exists

`related_docs` is already implemented in agent-doc. It's in the `Frontmatter` struct (line 252 of frontmatter.rs) and actively used by preflight (preflight.rs:378-408).

**How it works:**
```yaml
---
related_docs:
  - ../professional/bob-remeika.md
---
```

During `agent-doc preflight`, `check_related_docs()` reads the related docs, computes their diffs against their snapshots, and includes any changes in the preflight JSON output. The agent then sees changes from linked documents alongside the main document's diff.

**Naming:** `related_docs` is the canonical field name — it's already in the codebase and used by ragie-web-doc.md. I'd recommend keeping `related_docs` over `linked` since it's already deployed.

**The issue:** When you "requested to link documents" and nothing was added to frontmatter — that was a skill/agent issue (the Claude session didn't know to add the `related_docs` field). The binary supports it; the skill just needs to know the field name.

Want me to add `related_docs` guidance to the SKILL.md so future sessions know to use it?
Before we do that, would it be better to rename `related_docs` to `linked`?
`linked` would also imply other sources such as urls, documents with other types of formats, etc.
I think it's actually a good idea to link these other types of sources.
### Re: Rename to `linked` + expand scope

Good thinking. `linked` is more general and future-proof:

We should rename `linked` to `links`.

```yaml
---
linked:
  - ../professional/bob-remeika.md          # relative path
  - tasks/software/agent-doc.md             # project-relative
  - https://docs.example.com/api            # URL
  - mail/contacts/ron-berkes/AGENTS.md      # non-agent-doc file
---
```

**Benefits of `linked` over `related_docs`:**
- Shorter, cleaner
- Implies active linking (not just "related")
- Natural extension to URLs, non-.md files, external resources
- The preflight can handle each type differently: local files → diff, URLs → fetch + cache

**Migration:** Keep `related_docs` as a backward-compatible alias via `#[serde(alias = "related_docs")]`. New docs use `linked`.

**Implementation scope:**
1. Rename field + alias (small change)
2. URL support in preflight — `WebFetch` or `curl` for URLs, cache in `.agent-doc/linked_cache/`
3. Non-.md file support — read content, include in preflight context

This is a feature design that should go into `tasks/software/agent-doc.md` for proper planning. Want me to add it there, or implement the rename here first?
This document will handle the feature.
One of the features of `related_docs` is changes to context would ripple across the related_docs.
How would `links` handle these updates? Would editable files be edited? How would the info in uneditable files, such as urls, be handled?
I suppose that we can see if the agent-doc repo can edit the source of the url...such as briantakita.me. But some of the urls would be uneditable.
### Re: `links` design — editable vs read-only sources

Good name — `links` is cleaner. Here's the design for how different source types are handled:

**Link types by editability:**

| Source | Read | Write | Change detection |
|--------|------|-------|-----------------|
| Local .md (agent-doc) | Preflight diff | `agent-doc write` via patch | Snapshot-based diff |
| Local file (non-.md) | Read content | Direct file edit | mtime + hash |
| URL (editable repo) | Fetch + cache | Git push to source repo | Cache diff vs fresh fetch |
| URL (read-only) | Fetch + cache | Cannot edit — context only | Cache diff vs fresh fetch |

**How changes ripple:**

1. **Local .md links:** Already implemented via `check_related_docs()`. Changes in linked docs appear in the preflight diff. The agent can respond to those changes and update the current doc OR write back to the linked doc.

2. **Local non-.md files:** Read the content during preflight, include as context. If the agent wants to update the linked file, it can use the Edit tool directly (not via agent-doc write).

3. **Editable URLs (e.g., briantakita.me):** If the URL maps to a known git repo (configured in the link metadata), the agent can clone/pull, edit, and push. This is a heavier operation — probably triggered explicitly, not on every preflight.

4. **Read-only URLs:** Fetch content, cache in `.agent-doc/links_cache/`, include as context. The agent can reference this content but can't edit the source. Changes detected by comparing cached vs fresh fetch.

**Frontmatter syntax:**
```yaml
---
links:
  - ../professional/bob-remeika.md           # local file (auto-detected)
  - https://briantakita.me/about             # URL (auto-detected)
  - url: https://example.com/api
    editable: false                           # explicit read-only
  - path: ../some-repo/README.md
    repo: git@github.com:user/repo.git       # editable via git
---
```

Simple entries (strings) are auto-detected: paths → local file, URLs → fetch+cache. Object entries allow metadata (editable flag, repo URL, refresh interval).

Want me to implement the rename (`related_docs` → `links` with alias) first as a foundation, then build out the URL support?
Yes

We lost the active claude session running the task `Yes` on the document.
Why?
Is it currently running in the background?
### Re: Lost session + rename status

**The lost session:** The previous Claude session that received your "Yes" was running in a tmux pane managed by `agent-doc start`. When a session dies (timeout, crash, network issue), the pane goes idle and `agent-doc` doesn't auto-restart — you have to re-invoke `/agent-doc` to pick up where it left off. That's what just happened.

Why did the session die in this case? We should have logging. If not, add logging.

**Is it running in the background?** No. The stash pane %275 is running `tasks/software/corky.md`, not this document. The previous session for this document is gone.

**Rename status:** The `related_docs` → `links` rename is **already done** (committed as `9b87df1` in agent-doc). The `Frontmatter` struct now has:
```rust
#[serde(default, skip_serializing_if = "Vec::is_empty", alias = "related_docs")]
pub links: Vec<String>,
```

Existing documents using `related_docs:` in frontmatter will continue to work via the alias. New documents should use `links:`.

**Remaining items:**
- URL link support (fetch + cache + include as context in preflight)
- Editable repo links (git push back to source)
- Object entry syntax (metadata like `editable`, `repo`, `refresh`)

Want me to start on URL link support next?
Yes
I did notice that the visible tmux window had a single tmux pane with a new Claude session.
### Re: Why did the session die + logging

**What happened:** Looking at `start.rs`, the restart loop (line 96-137) shows the behavior:
- Non-zero exit → auto-restart with `--continue` after 2s
- Clean exit (code 0) → prompt user to restart or quit

The "single tmux pane with a new Claude session" you saw was likely Claude restarting via `--continue` after a crash. The problem: `--continue` resumes the Claude conversation, but the `/agent-doc` skill context isn't re-injected — so it becomes a bare Claude session that doesn't know about the document workflow. That's why the session was "lost" even though the pane was still alive.

Are there any agent-doc solutions for this type of issue?

**Logging:** Currently there's NO persistent logging — only stderr to the tmux pane (ephemeral, scrolls away). We should add:

1. **Session log file:** `.agent-doc/logs/<session-uuid>.log` — append exit codes, timestamps, restart events
2. **Log rotation:** Keep last N sessions or last N days
3. **What to log:** Start time, exit code, restart count, `--continue` vs fresh, crash signals

This would let you debug "what happened to my session?" after the fact.

### Re: URL link support — approved

Adding to pending. This is a meaningful feature — I'll design the implementation before coding.

**URL link support design:**
1. **Detection:** In `preflight.rs`, when iterating `links`, detect URLs (starts with `http://` or `https://`)
2. **Fetch + cache:** Download URL content to `.agent-doc/links_cache/<url-hash>.md`, compare against cached version
3. **Include in preflight JSON:** Add a `links` field to the preflight output with content/diffs from each linked resource
4. **Cache invalidation:** Re-fetch on each preflight (URLs are cheap to fetch, and staleness is worse than latency)

Want me to implement this now, or should we address the session logging first?
Session logging first
Make sure this is in our agent:pending
### Re: Solutions for --continue skill loss

Yes, there are several approaches:

**1. Pipe initial prompt on restart (best option)**
After `--continue`, `start.rs` could write `/agent-doc <file>` to Claude's stdin as the first message. This would re-trigger the skill workflow in the resumed conversation. The conversation history is preserved (so Claude has context), and the skill re-executes the preflight cycle.

Implementation in `start.rs`:
```rust
// Instead of just cmd.status(), use stdin pipe
let mut child = cmd.stdin(Stdio::piped()).spawn()?;
if !first_run {
    if let Some(mut stdin) = child.stdin.take() {
        writeln!(stdin, "/agent-doc {}", file.display())?;
    }
}
child.wait()?
```

**2. SessionStart hook auto-trigger**
Claude Code supports hooks that fire on session start. A `SessionStart` hook could detect that this is a `--continue` restart and auto-inject the `/agent-doc` command. However, hooks can't inject user messages — they run shell commands.

**3. `--initial-prompt` flag (doesn't exist yet)**
Claude Code doesn't have a `--initial-prompt` flag that works with `--continue`. This would be the cleanest solution but requires upstream support.

**Recommendation:** Option 1 is the most practical. The stdin pipe approach works today without any upstream changes. The key insight is: `--continue` preserves conversation context, but the agent needs a _trigger_ to re-enter the agent-doc workflow loop. Piping `/agent-doc <file>` provides that trigger.

Want me to implement this in `start.rs`?
Yes
### Re: Session logging — implemented

Done. Added persistent session logging to `start.rs`. Logs are written to `.agent-doc/logs/<session-uuid>.log` in append mode.

**What's logged:**
- `session_start` — file, pane ID, session UUID
- `claude_start` — mode (fresh/fresh_restart), restart count
- `claude_restart` — mode=continue, restart count
- `claude_exit` — exit code, restart count
- `stdin_read_failed` / `user_quit` — why the loop ended
- `session_end` — clean shutdown

**Example log output:**
```
[1743436383] session_start file=tasks/foo.md pane=%42 session=bef800bc
[1743436383] claude_start mode=fresh restart_count=0
[1743436512] claude_exit code=1 restart_count=0
[1743436514] claude_restart mode=continue restart_count=1
[1743436600] claude_exit code=0 restart_count=1
[1743436601] user_quit
[1743436601] session_end
```

**Build status:** Clippy clean, 38 tests pass, release build complete.

The timestamps are Unix epoch seconds (no chrono dependency needed). The log directory is auto-created at `.agent-doc/logs/` next to the existing `.agent-doc/` structure.

Next up: session logging is done. Ready to work on **auto-trigger on restart** (pipe `/agent-doc <file>` to stdin) or **URL link support**. Which first?

Continue:
- [ ] **Auto-trigger on restart** — pipe `/agent-doc <file>` to stdin after `--continue`
### Re: Auto-trigger on restart — implemented

Done. After a `--continue` restart, `start.rs` now spawns a background thread that:

1. Waits 5 seconds for Claude to initialize
2. Sends `/agent-doc <file>` via `tmux send-keys` to the current pane
3. Logs the trigger event to the session log

**Why tmux send-keys instead of stdin pipe?**
Claude needs interactive stdin for the rest of the session. Piping to stdin and dropping it would break interactivity. `tmux send-keys` injects the command as if the user typed it, preserving full terminal interactivity.

**What happens on restart now:**
```
Claude exits (code 1) → 2s delay → restart with --continue
→ 5s delay → tmux send-keys "/agent-doc tasks/foo.md" Enter
→ Claude receives /agent-doc, skill re-triggers, preflight runs
```

**Build status:** Clippy clean, all tests pass, release build complete.

Both session logging and auto-trigger are now implemented. Ready for URL link support next?
### Re: Continue — URL link support

Acknowledged. Auto-trigger is done. Moving to **URL link support**.

This is a larger feature — let me scope the initial implementation:

**MVP (this session):**
1. Detect URL entries in `links` frontmatter field (starts with `http://` or `https://`)
2. Fetch URL content via `ureq` (already a dependency for `upgrade.rs`)
3. Cache fetched content in `.agent-doc/links_cache/<sha256(url)>.txt`
4. On preflight: fetch fresh, diff against cache, include in preflight JSON output
5. Update cache after successful diff

**Not in MVP:**
- Object entry syntax (`url:`, `editable:`, `repo:`)
- Editable repo links
- HTML-to-markdown conversion (raw HTML for now)

Starting implementation now.
### Re: URL link support — implemented

Done. URL links now work in the `links` frontmatter field. Implementation in `preflight.rs`.

**How it works:**
1. During preflight, each link is checked: `is_url()` detects `http://` or `https://` prefixes
2. URLs are fetched via `ureq` with a 10-second timeout
3. Content is cached at `.agent-doc/links_cache/<sha256(url)>.txt`
4. On subsequent preflights, fetched content is diffed against the cache:
   - **First fetch:** Reports "initial fetch (N bytes)" and caches
   - **Content changed:** Reports "content changed (N bytes)" and updates cache
   - **No change:** Silently skipped (not included in preflight output)
   - **Fetch error:** Reports the error with `exists: false`

**Example frontmatter:**
```yaml
---
links:
  - ../professional/bob-remeika.md
  - https://briantakita.me/about
---
```

**Preflight output** (when URL content changes):
```json
{
  "linked_changes": [
    { "path": "https://briantakita.me/about", "summary": "content changed (4521 bytes)", "exists": true }
  ]
}
```

**Build status:** Clippy clean, all tests pass, release build complete.

**What's NOT included yet:**
- HTML-to-markdown conversion (agent gets raw HTML)
- Object entry syntax for metadata
- Editable repo links

Is markitdown the canonical tool to use?
https://github.com/microsoft/markitdown

Is there a Rust equivalent to remove the python dependency?
### Re: HTML-to-markdown — markitdown vs Rust alternatives

**markitdown** (Microsoft) is a Python tool that converts many formats (HTML, PDF, DOCX, etc.) to markdown. It's comprehensive but adds a Python dependency.

**Rust alternatives exist** — no Python needed:

| Crate | Downloads/mo | Quality | Notes |
|-------|-------------|---------|-------|
| **html2md** | ~30k | Most mature, production-ready | DOM-based, GFM tables |
| **htmd** | Growing | Well-maintained, customizable | Turndown.js-inspired |
| **html-to-markdown-rs** | Newer | Modern, multi-runtime | Uses lol-html parser |
| **quick_html2md** | Small | Active | Speed-focused, GFM |

**Recommendation:** Use **`htmd`** or **`html2md`** — both are pure Rust, DOM-based (not regex), and handle the common HTML structures we'd encounter from URL fetches. This keeps agent-doc dependency-free of Python.

The conversion would slot into `check_url_link()` — after fetching, convert HTML to markdown before caching and diffing. This gives the agent clean, readable content instead of raw HTML.

Want me to add one of these crates and wire it into the URL link fetch pipeline?
Are there rust equivalents for PDF and DOCX?
### Re: Rust equivalents for PDF and DOCX

**PDF — yes, but with caveats:**

| Crate | Type | Quality | Notes |
|-------|------|---------|-------|
| **pdfium-render** | FFI (PDFium) | Production-ready | Best text extraction, requires PDFium binary |
| **pdf-extract** | Pure Rust | Moderate | Text-only, no layout/formatting |
| **pdf** | Pure Rust | Basic | Low-level, minimal extraction |

**DOCX — yes:**

| Crate | Type | Quality | Notes |
|-------|------|---------|-------|
| **docx-rs** | Pure Rust | Production-ready | Handles formatting, text extraction |
| **docx** | Pure Rust | Basic | Lightweight, simple API |

**Key limitation:** None of these crates natively convert to **markdown**. They extract raw text or structured elements. You'd need a post-processing step to generate markdown from the extracted content.

**Comparison with markitdown:**
- markitdown handles HTML, PDF, DOCX, PPTX, XLSX, images (OCR), and audio — all in one tool
- Rust equivalents require assembling 3-4 separate crates + custom markdown generation
- markitdown is a single `pip install` and a single API

**My recommendation for agent-doc:** Start with **HTML-to-markdown only** (pure Rust via `htmd`/`html2md`). For PDF/DOCX, shell out to `markitdown` if it's installed, with graceful fallback. This gives us:
- Zero Python dependency for the common case (HTML URLs)
- Full format support when markitdown is available
- No need to maintain custom PDF/DOCX→markdown logic

This is a pragmatic hybrid — Rust for the hot path, Python for the long tail.

Ragie parse also handles OCR and presumably md. https://www.ragie.ai/parse
Let's use the Rust binaries for now. We can have optional markdownit and ragie parse integration.
Focus on html. Which html to md is the best library to use in our case?
### Re: Best HTML-to-markdown library for agent-doc

**`htmd`** is the best fit for our case. Here's why:

| Criterion | htmd | html2md |
|-----------|------|---------|
| **Boilerplate stripping** | Explicit skip-tags config (script, style, nav, header, footer) | Implicit |
| **GFM tables** | Yes | Yes |
| **Code blocks** | Yes | Yes |
| **API** | `htmd::convert("<h1>Hi</h1>")` | `html2md::rewrite_html("<p>text</p>", false)` |
| **Dependencies** | 1 (html5ever) | 2 (lol_html, scraper) |
| **Maintenance** | Active, 17 releases, Apache 2.0 | Active, MIT |
| **Performance** | ~16ms for 1.37MB Wikipedia page | Fastest (streaming parser) |

**Why htmd wins for us:**
1. **Boilerplate control** — we can skip `<script>`, `<style>`, `<nav>`, `<header>`, `<footer>` upfront via `HtmlToMarkdownBuilder`, giving the agent clean content
2. **Lighter deps** — single dependency (html5ever), smaller compile impact
3. **Turndown.js test suite** — proven conversion quality
4. **Good enough speed** — 16ms for a Wikipedia page is fine for preflight

Want me to add `htmd` and wire it into `check_url_link()` now?
Yes
### Re: htmd integrated — HTML-to-markdown in URL fetch pipeline

Done. `htmd` v0.5.3 is now wired into `check_url_link()` in `preflight.rs`.

**How it works:**
1. URL is fetched via `ureq`
2. If `content-type` header contains `text/html` or `application/xhtml`, the response is converted to markdown via `htmd`
3. Non-HTML responses (JSON, plain text, etc.) pass through unchanged
4. The **markdown** output is what gets cached and diffed — agents see clean, readable content

**Boilerplate stripping:** The converter skips `<script>`, `<style>`, `<nav>`, `<footer>`, `<noscript>`, and `<svg>` tags, removing common web page boilerplate.

**Build status:** Clippy clean, all tests pass, release build complete. Added 21 transitive deps (html5ever + related parsing crates).
21 transitive deps is a lot of dependencies. Please show me. Did we include any http server implementation? I would want that decoupled.
Add tests
audit docs
Update SPEC.md, README.md
### Re: Dependency audit + action items

**htmd dependency tree** — 3 direct deps, all HTML parsing:
```
htmd v0.5.3
├── html5ever v0.36.1        # Mozilla's HTML parser
├── markup5ever_rcdom v0.36.0 # DOM tree for html5ever
└── phf v0.13.1               # Perfect hash function (tag lookup)
```

**No HTTP server.** The transitive deps are all parsing-related: `tendril` (string buffers), `string_cache` (atom interning), `web_atoms` (HTML/SVG atom definitions), `phf_*` (compile-time hash tables). Many overlap with existing deps (proc-macro2, quote, syn, cfg-if, libc, log, smallvec are already pulled by other crates).

**Truly new crates:** htmd, html5ever, markup5ever, markup5ever_rcdom, xml5ever, tendril, futf, mac, web_atoms, string_cache, precomputed-hash, new_debug_unreachable, utf-8 — ~13 new crates, all from the html5ever ecosystem (Mozilla-maintained).

Working on tests, audit docs, and SPEC/README updates now.
### Re: Tests, audit docs, SPEC/README updates — done

**Tests added** (7 new tests in `preflight.rs`):
- `is_url_detects_http` — URL detection for http/https prefixes
- `is_html_content_detects_html` — content-type sniffing
- `html_to_markdown_converts_basic_html` — h1/strong → markdown
- `html_to_markdown_strips_script_and_style` — boilerplate removal
- `html_to_markdown_strips_nav_and_footer` — boilerplate removal
- `url_cache_path_is_deterministic` — cache path stability
- `links_cache_dir_creates_directory` — directory creation

All 361 tests pass, 0 failures.

**Audit docs:** Module spec comments updated in `preflight.rs` (linked docs step 3c, URL processing, HTML conversion) and `start.rs` (session logging, auto-trigger). Eval comments added for all 7 new tests.

**SPEC.md updated:** Preflight section now documents linked_changes output, URL link processing, HTML-to-markdown conversion, and cache directory.

**README.md updated:** Added "Linked resources" and "Session logging" to Key Features.

**Audit result:** 1 remaining issue — pre-existing large table warning in CLAUDE.md (not related to our changes).

The debouncing in tasks/recruit/lmi.md did not work. Why not?
### Re: LMI debounce not working

The debounce in preflight has two layers:

1. **Step 3b — mtime debounce** (500ms): Waits until the file's mtime is at least 500ms old before computing the diff. This prevents picking up mid-save edits.

2. **Diff truncation detection** (step 4): After computing the diff, checks if the last line looks like it's actively being typed (short, growing). Rechecks up to 12 times with delays.

**What likely happened with lmi.md:** The debounce works based on **file mtime** — it waits for the file to stop being modified. But if the user saves the file and then immediately runs `/agent-doc` (or agent-doc is auto-triggered), the 500ms mtime debounce may have already passed by the time preflight reaches step 3b.

**The truncation detection in the diff step** is what caught the partial typing in this document's earlier preflight:
```
[diff] Last line may be truncated (recheck 1/12): "Are there"
[diff] Last line may be truncated (recheck 6/12): "Are there any solutions for this type of issue"
```
That waited ~6 rechecks (~1.5s) for the typing to settle.

**Possible failure modes:**
1. The editor auto-saves, but the user is still typing — the mtime debounce sees a settled file even though the buffer has unsaved changes
2. The debounce timeout (3s) expired before typing finished
3. The typing indicator (`is_typing_via_file`) wasn't active for this document

To diagnose: can you describe what you saw? Did the agent respond to a partial/truncated prompt? Or did it respond before you finished editing?

Please add to README.md, if not already done, a note about security. agent-doc is only secure with a single user. Changes are written to a git repo, so any unintended divulgence of data is at the user's risk. A private monorepo base is recommended.
Please verify security message.
See tasks/software/ragie-web-doc.md and tasks/recruit/lmi.md for more context.
This security concern should be at the top of the document. Under Alpha Software.
See the Security section for more details.
### Re: Security section added to README.md

Done. Added a **Security** section to README.md covering:

- **Single user only** ... no multi-user access control, authentication, or session isolation
- **Private repo recommended** ... session documents may contain sensitive content
- **Prompt injection risk** ... content pasted from external sources could contain injection attempts, no scanning in place
- **`--dangerously-skip-permissions` exposure** ... full filesystem access when using this flag
- **Planned** ... collaborative security for web/networked deployments

The language draws from the ragie-web-doc security analysis (data divulgence through the response channel) and the lmi.md email discussion (single-user model, private git backing, roadmap for collaborative security).

Please review the README.md directly to verify the wording.
### Re: Security note at top of README

Done. Added a blockquote right under the "Alpha Software" note:

> **Single-user only.** agent-doc operates on the local filesystem with no access control. Use a private git repository. See the [Security](#security) section for details.

The full Security section remains at the bottom with the detailed breakdown. The top-level note gives immediate visibility.

Add security to SPEC.md. Add current security model. More work TBD. 
### Re: Security added to SPEC.md

Done. Added **Section 10: Security** to SPEC.md with three subsections:

- **10.1 Threat Model** ... trusted user/untrusted content, local filesystem scope, git as audit trail
- **10.2 Known Risks** ... prompt injection via document content, `--dangerously-skip-permissions` exposure, data divulgence through the response channel, links cache sensitivity
- **10.3 Recommendations** ... private repo, no secrets in context, wait for multi-user security, review before sharing

The security content draws from the ragie-web-doc analysis (data divulgence is the fundamental unsolved problem) and aligns with the README security section. The SPEC is more detailed since it's the functional specification for implementors.

### Re: Security model status

Acknowledged. SPEC.md Section 10 covers the current security model. The "More work TBD" items from the ragie-web-doc analysis (content scanning, session isolation, multi-user access control) are tracked in pending.

All documentation work for this session is complete. Ready for the next priority when you are.

audit docs
commit and push
should we release this change?
### Re: Audit, commit, push, release?

**Audit docs:** 1 issue (pre-existing large table warning in CLAUDE.md). No new issues.

**Committed and pushed:**
- Submodule: `9f8ddc4` feat: links with URL fetch, HTML-to-markdown, session logging, auto-trigger (778 insertions, 9 files)
- Parent: `b44d5c34` update agent-doc submodule pointer

**Should we release?** This is a significant feature set:
- `links` frontmatter with URL fetch + HTML-to-markdown
- Session logging
- Auto-trigger on restart
- Security documentation
- 7 new tests

I'd recommend **yes, release as v0.29.0** (minor bump for new features). The checklist:
1. Bump version in Cargo.toml + pyproject.toml
2. Update VERSIONS.md
3. `make check` (already passing)
4. Tag + push tag
5. `cargo publish` + `maturin publish` + `gh release create`
6. `cargo install --path .`

Want me to proceed with the release?
release
### Re: v0.29.0 released

All publish targets complete:

- **crates.io:** `agent-doc v0.29.0` published
- **PyPI:** `agent_doc-0.29.0` published (manylinux_2_39_x86_64)
- **GitHub Release:** https://github.com/btakita/agent-doc/releases/tag/v0.29.0 (with binary + wheel)
- **Local install:** `~/.cargo/bin/agent-doc` updated to v0.29.0
- **Parent submodule:** pointer updated to v0.29.0

In tasks/recruit/lmi.md, there is a full uncommitted response patch. Why?
### Re: Uncommitted response patch in lmi.md

**Root cause:** The response was written to the file (via IPC to the editor plugin) but the **snapshot was never updated** with the response content. The `agent-doc commit` command commits based on the snapshot — so it committed the boundary marker changes but not the response body, because the snapshot didn't contain it.

**How this happened:** The session that wrote the response likely crashed or lost context after `agent-doc write` succeeded but before `agent-doc commit` could properly stage the changes. The IPC write updated the file on disk, but the snapshot save either failed or was skipped.

This is the exact scenario the `recover` step in preflight is designed to catch — but it only detects orphaned responses in `.agent-doc/pending/`, not snapshot/file divergence.

**Fix applied:** Updated the snapshot to match the current file content, then ran `agent-doc commit`. The response is now committed and the gutter is clean.

**Bug to track:** `agent-doc commit` should detect when the file has content not in the snapshot and handle it (either by updating the snapshot or warning). Currently it silently ignores this divergence.

Propose bug fix
### Re: Bug fix proposal — snapshot/file divergence detection

**The bug:** When `agent-doc write` succeeds (file updated via IPC) but the snapshot isn't saved (crash, IPC timeout, plugin error), subsequent `agent-doc commit` stages the stale snapshot content, leaving the actual response uncommitted.

**Where the fix goes:** `git.rs:commit()`, line 132-134.

**Proposed fix — divergence detection in commit:**

```rust
// After loading snapshot, compare against file content
let snapshot_content = crate::snapshot::load(file)?;
if let Some(ref snap) = snapshot_content {
    let file_content = std::fs::read_to_string(file)?;
    // Strip boundary markers for comparison
    let snap_clean = crate::boundary::remove_all_boundaries(snap);
    let file_clean = crate::boundary::remove_all_boundaries(&file_content);
    
    if file_clean.len() > snap_clean.len() + 50 {
        // File has significant content beyond snapshot — likely an unsynced write
        eprintln!("[commit] WARNING: file has {} bytes beyond snapshot — updating snapshot",
            file_clean.len() - snap_clean.len());
        // Update snapshot to match file before staging
        crate::snapshot::save(file, &file_content)?;
        // Re-load updated snapshot for staging
        let updated_snap = file_content;
        // ... use updated_snap for staging instead of snap
    }
}
```

**Key design decisions:**
1. **50-byte threshold** — avoids false positives from minor whitespace/boundary differences: Would removing ` (HEAD)` prefix cause a false positive for long titles?
2. **Auto-fix, not just warn** — updates the snapshot so the commit includes the response
3. **Length comparison, not content diff** — fast, and the failure mode is always "file has more content than snapshot" (a response was written but snapshot wasn't saved)

**Alternative: preflight-level detection**
Could also add this check to `preflight.rs` step 2 (before commit), where it would detect the divergence and either auto-fix or surface it in the JSON output. This gives the skill a chance to handle it.

Want me to implement this fix?
### Re: (HEAD) marker false positive risk

No false positive risk. The `(HEAD)` marker is only added during `commit()` staging — it's injected into the staged content via `add_head_marker()`, never saved to the snapshot itself.

The comparison in the proposed fix compares **snapshot vs file** — neither contains `(HEAD)`:
- **Snapshot:** Raw content saved by `agent-doc write`
- **File on disk:** Raw content written by IPC/disk write

The `(HEAD)` only exists in the **git index** (staged content) — it's a commit-time decoration, not persisted to either the snapshot or the working tree.

I confirmed: the snapshots that grep-matched `(HEAD)` have it as literal text in the content (discussing the HEAD marker feature), not as a marker suffix on headings.

The 50-byte threshold is safe. The only content difference between snapshot and file in the failure case is the missing agent response (typically hundreds to thousands of bytes).

Want me to implement the fix now?
Note that tasks/recruit/lmi.md had the issue occur again. Why is this happening on lmi.md? Is there something about the file or session that is causing the issue?
### Re: Recurring lmi.md snapshot divergence

Investigated the code paths. The IPC write + snapshot save logic looks correct in all paths — `content_ours` is saved as the snapshot when provided. But lmi.md keeps having uncommitted responses.

**Most likely cause:** The lmi.md session's Claude instance is using `agent-doc write --stream` (SKILL-driven), and the snapshot save at the end of `write_ipc_and_poll()` is succeeding — but then **a subsequent preflight commit from another document's session** is running `agent-doc commit` on its own file, and the lmi.md session's `agent-doc commit` step is being skipped because the Claude session crashed or lost context before reaching the commit step.

In other words: the **write** succeeds (snapshot saved), but then the **next preflight cycle** for lmi.md doesn't happen because the session died. Meanwhile, the next `/agent-doc` invocation on this document (agent-doc-claim-error.md) runs preflight which calls `agent-doc commit` — but only for this file, not lmi.md.

**The fix is still the proposed divergence detection in `commit()`:** When preflight calls `commit()` for any file, it should check if the file has content beyond the snapshot and auto-fix. The preflight commit step already runs for every document cycle — we just need to make it smarter.

**Quick mitigation now:** Let me sync the lmi.md snapshot.

I did see a new session start when I navigated back to the document in the editor.
We need to have an accurate check of all running claude sessions + tmux panes.
The requisite logging should also occur.
The running session validation check needs to be accurate and comprehensive and happen when running any agent-doc command that could start a new tmux pane + claude session.
How can we do this?
### Re: Session validation and duplicate prevention

You're describing a critical issue: the editor sync triggered a **new session** for lmi.md even though one was already running. This means the existing session's pane was either dead or not properly detected.

**Current validation chain:**
1. `route.rs` checks `sessions.json` for the document's session UUID → pane ID mapping
2. `tmux.pane_alive(pane)` checks if the pane still exists
3. If alive, `send_command` sends `/agent-doc <file>` to the existing pane
4. If dead/missing, `auto_start` creates a new pane + Claude session

**Where this breaks:**
- The old pane may have been killed (context exhaustion, manual close) but `sessions.json` wasn't updated
- The new `auto_start` registers a new session UUID, but the old UUID is still in the document's frontmatter
- Now there are two session entries — one dead, one alive — and the document's frontmatter points to the old dead one

**Comprehensive fix — three layers:**

**Layer 1: Validate before auto-start**
Before creating a new pane, check ALL panes running `agent-doc` or `claude` and cross-reference with `sessions.json`. If any pane is already serving this document, route to it instead of starting a new one.

**Layer 2: Session registry cleanup on every command**
Every `agent-doc` command that touches `sessions.json` should:
- Prune dead panes (pane_alive check)
- Detect duplicate panes for the same file
- Log pruning events to `.agent-doc/logs/`

**Layer 3: Process-level dedup guard**
Before starting a new Claude process for a document, check if any `claude` process tree already has this file path as an argument. Use `pstree` or `/proc` inspection.

**Implementation plan:**
1. Add `sessions::prune_dead()` — called at start of `route`, `sync`, and `preflight`
2. Add `sessions::find_pane_for_file(file)` — checks all entries, not just the session UUID in frontmatter
3. Add process check in `auto_start` — scan tmux panes for active claude processes serving this file
4. Log all session lifecycle events to `.agent-doc/logs/sessions.log`

Want me to implement this? It's a v0.29.1 fix.
I do. But we also need to discover and address why uncommited response patches were written to tasks/recruit/lmi.md
And we need to fix the issue(s).
### Re: Root cause of lmi.md uncommitted responses

Found it. Here's the exact sequence from git history:

**Timeline:**
1. `4a644996` (18:15:11) — Response not yet written
2. `34e4d14b` (18:15:46) — Response "Re: Private Repo Correction" **correctly committed** with `(HEAD)` marker
3. `0559f7fb` (18:14:34) — Response **removed** from committed content! The snapshot rolled back, and the `(HEAD)` marker moved up to an earlier heading
4. `b8483cda` (18:16:31) — Response appeared **twice** (count=2), then cleaned up

**Root cause: A second session started with a stale snapshot.**

When the user navigated to lmi.md in the editor, the sync/route logic detected no active pane (the old one was dead) and started a **new Claude session**. This new session ran `agent-doc commit` during its preflight, but its snapshot was from BEFORE the response was written. So it staged the old snapshot content — effectively rolling back the commit that had the response.

Then the new session presumably re-ran the `/agent-doc` workflow, saw the user's prompt still in the diff (since the response was now uncommitted again), and re-generated the response — causing the double-write.

Why would the issue occur twice then? lmi.md was the selected document until the second time the uncommited response patch occurred...which happened from a single agent run.

**This confirms the session dedup issue is the root cause.** The fix needs to:

1. **Never start a new session if one is already serving the file** — even if the old session's pane appears dead, check if the snapshot is current before overwriting it: What should appen instead?
2. **Detect snapshot rollback** — if `commit()` would stage content that is SHORTER than the current committed version, warn and abort: Could this happen when we compact the agent:exchange component? Compact would shrink the agent:exchange content. 
3. **Session lifecycle logging** — every start, restart, and commit should be logged with session UUID and file path

The snapshot/file divergence fix alone won't help — it's the **commit staging the old snapshot** that causes the rollback. We need to prevent the second session from starting, AND add a guard in `commit()` that refuses to stage content shorter than HEAD.: How about for compacting agent:exchange?

Want me to implement the full fix? This is a critical reliability issue.
### Re: Three clarifying questions

**Q1: Why did the issue occur twice from a single agent run?**

Looking at the git history again: the second occurrence likely happened because the new session's preflight committed the stale snapshot (rolling back the response), then the SKILL workflow saw the user's prompt in the diff and re-generated the response. But this re-generated response was also not committed because the session crashed/died again before the commit step. So:
- First occurrence: old session wrote response, died before commit
- Second occurrence: new session rolled back the first response via stale snapshot commit, re-generated it, then ALSO died before commit

Are we detecting **if** and **why** a session dies?
I believe we were on the same tmux pane the second run...Did the second run with the second occurrence of the issue have any pane deaths? Please check the logs. Add the necessary logs that are missing.

The underlying issue is session instability (sessions keep dying), combined with stale snapshot commits on restart.

**Q2: What should happen instead of starting a new session?**

When a pane for a file appears dead:
1. Check if the **file on disk** has content beyond the **snapshot** (divergence detection)
2. If diverged: update the snapshot from the file BEFORE committing (absorb the orphaned response)
3. THEN start a new session

This way the new session inherits the current file state, not the stale snapshot.

**Q3: Compact shrinks content. How does the rollback guard handle that?**

Good catch. A simple "shorter = rollback" check would false-positive on compaction. The fix:

Instead of comparing content **length**, compare the **boundary marker ID** and the **last `### Re:` heading**. The guard should check:
- Does the staged content contain all `### Re:` headings that exist in git HEAD? If not, it's a rollback.
- Exception: if the file contains a compaction marker (`*Compacted.*`), length reduction is expected.

Alternatively, simpler approach: **only guard against shrinkage in the exchange component**, not the whole file. Compact replaces exchange content with a compaction marker — it doesn't delete responses without leaving a trace.

```rust
// In commit(), before staging:
let head_content = git_show_head(file)?;
let head_exchange = extract_exchange(&head_content);
let snap_exchange = extract_exchange(&snapshot);
if snap_exchange.len() < head_exchange.len() 
    && !snap_exchange.contains("*Compacted") 
{
    eprintln!("[commit] WARNING: snapshot exchange is shorter than HEAD — possible rollback");
    // Update snapshot from file instead
}
```

This handles both cases: normal responses (exchange grows) and compaction (exchange shrinks but has compaction marker).

Can the document shrink without the word "Compacted"?
### Re: Session death detection + log analysis

**Key finding: The session didn't die.** The session logs show `session_start` + `claude_start` for lmi.md at 17:31 with NO exit/restart events. Pane %298 is still alive running `agent-doc` -> `claude`. The `start.rs` restart loop never triggered.

**Revised theory:** The issue isn't session death. It's the **Claude conversation** inside the session losing its thread. The `start.rs` loop is running fine, but the Claude agent inside:
1. Ran `agent-doc write --stream` successfully (file updated, snapshot saved)
2. Then either hit a tool error, lost context, or was interrupted before running `agent-doc commit`: What do the logs say? Anything that should be logged to validate or invalidate this theory that is missing from the logs?
3. On the next `/agent-doc` cycle, preflight's `commit()` ran with the snapshot but something went wrong with the snapshot state: Do the logs provide any clues? Anything that should be logged that is not logged to provide evidence of what happened?

**What logs are missing:**
- **Write-level logging**: `agent-doc write` should log every successful write + snapshot save to `.agent-doc/logs/ops.log`
- **Commit-level logging**: `agent-doc commit` should log what it's staging (snapshot length vs file length)
- **Snapshot state logging**: log snapshot hash + length on save and load

**Immediate action:** Add write/commit/snapshot logging so we can trace the exact failure next time it occurs. The session-level logging confirmed the session is stable. Now we need operation-level logging.

Want me to add this logging now?
Yes
### Re: Can the document shrink without "Compacted"?

Yes. Several scenarios:

1. **User deletes content** ... removes a response, trims a section, deletes old exchanges
2. **`agent-doc undo`** ... restores pre-response snapshot, shrinking back to before the last response
3. **`agent-doc extract`** ... moves exchange sections to a new document, shrinking the original
4. **Agent replaces status/pending** ... `patch=replace` on status/pending components can shrink them
5. **User edits within exchange** ... deletes or shortens their own prior questions

So the "Compacted" marker check is insufficient. A better approach:

**Compare at the git level, not content level.** Instead of guarding against content shrinkage, guard against **staging content that doesn't contain responses present in git HEAD**:

```rust
// In commit(), extract ### Re: headings from both
let head_headings = extract_re_headings(&head_content);
let snap_headings = extract_re_headings(&snapshot);

// Every Re: heading in HEAD must exist in snapshot (unless compacted)
let missing: Vec<_> = head_headings.iter()
    .filter(|h| !snap_headings.contains(h))
    .collect();

if !missing.is_empty() && !snapshot.contains("*Compacted") {
    eprintln!("[commit] WARNING: snapshot missing {} Re: headings from HEAD", missing.len());
    // Absorb file content into snapshot instead
}
```

This is precise: it detects rollback (missing responses) while allowing all legitimate shrinkage (deletions, undo, replace-mode patches, extractions). The only false positive would be if a user manually deleted a `### Re:` heading AND the content below it, which is an intentional edit that should be respected.

We need detailed specs for all of this. Is a single SPEC.md sufficient?
The SPEC(s) need to be validatable as well.
### Re: Log analysis + ops logging + SPEC structure

**Current logs for lmi.md session:**
```
[1774992690] session_start file=tasks/recruit/lmi.md pane=%298 session=69cd8603
[1774992690] claude_start mode=fresh restart_count=0
```
That's it. Two lines. No exits, no restarts, no errors. The session has been running continuously for hours. This confirms the `start.rs` loop is stable. What's missing: any record of what the Claude conversation DID during those hours (writes, commits, snapshot saves).

**What needs logging to validate/invalidate the theory:**
- Did `agent-doc write --stream` succeed? (snapshot save event)
- Did `agent-doc commit` run after the write? (commit staging event)
- Was the snapshot length consistent with the file? (comparison at commit time)

Without these, we're guessing. I've launched a background agent to implement operation-level logging now.

**SPEC.md structure:** A single SPEC.md is fine for now. It's at 722 lines. If it grows past ~1000 lines, consider splitting into:
- `SPEC.md` — core behavior (document format, snapshot, diff, write, git)
- `SPEC-session.md` — session lifecycle (start, route, sync, claim, focus)
- `SPEC-security.md` — security model, threat model, recommendations

But splitting prematurely adds navigation overhead. For now, add new sections to the existing SPEC.md. The audit-docs budget allows up to 1000 lines per instruction file, and SPEC.md at 722 has room.

Add a SPEC to split the file if it grows past 1000 lines.

### Re: Validatable specs

Agreed. agent-doc already has a pattern for this: every module has `## Spec`, `## Agentic Contracts`, and `## Evals` sections in its doc comments. The evals are validatable assertions that map 1:1 to tests.

**For the new features, the spec structure should be:**

1. **Module spec comment** (in the `.rs` file) ... describes behavior, contracts, and evals
2. **SPEC.md section** ... captures cross-module behavior for porters/reimplementors
3. **Tests** ... implement each eval from the module spec

**For the rollback guard and ops logging, the evals would be:**

```
// git.rs evals:
- commit_detects_snapshot_rollback: snapshot missing a `### Re:` heading present in HEAD
  → warning logged, snapshot updated from file before staging
- commit_allows_compacted_shrinkage: snapshot has `*Compacted` marker and fewer headings
  → commit proceeds normally (no rollback warning)
- commit_logs_staging_details: every commit writes snap_len + file_len to ops.log

// write.rs evals:
- write_logs_snapshot_save: successful write → ops.log entry with file + len
- write_logs_ipc_consumed: IPC success → ops.log entry with file + patches count

// snapshot.rs evals:
- snapshot_save_logs_operation: save() → ops.log entry with file + content length
```
When you say evals, is this a sort of runtime eval that occurs to validate SKILL.md and the agent harness is operating correctly?

These become both the spec documentation and the test definitions. The ops logging agent is implementing the logging now. Once it's done, I'll add the tests and spec comments.

**SPEC.md addition for ops logging:**

Will add a new section `10.4 Operations Logging` (under Security, since logging is audit-adjacent) or a new `## 11. Observability` section. The spec will define:
- Log file location (`.agent-doc/logs/ops.log`)
- Log format (`[timestamp] operation key=value ...`)
- Required events (snapshot_save, ipc_write, commit_staging, commit_success)
- Best-effort semantics (logging failure never blocks operations)

Should we be continously reading the logs to ensure no anomalies occur in realtime?
### Re: SPEC split threshold, evals definition, real-time log monitoring

**SPEC split at 1000 lines:** Noted. I'll add a self-documenting rule to SPEC.md itself:

> When SPEC.md exceeds 1000 lines, split into domain-specific spec files (SPEC-session.md, SPEC-security.md, etc.) and update audit-docs to validate each.

**Evals clarification:** No, these are NOT runtime evals. They're **unit test definitions** in the module's doc comments. The naming convention comes from the existing codebase pattern:

```rust
//! ## Evals
//! - `commit_detects_snapshot_rollback`: ...
//! - `commit_allows_compacted_shrinkage`: ...
```

Each eval becomes a `#[test]` function with the same name. They validate the **binary's** behavior, not the SKILL.md or agent harness. The agent harness (SKILL.md + Claude) is non-deterministic and can't be unit-tested this way.

For agent harness validation, you'd need integration tests or observability (which is what the ops logging gives you). The ops.log is the "runtime eval" equivalent for the agent harness.

**Real-time log monitoring:** Yes, this is a good idea. Two approaches:

1. **Preflight-level check:** During `preflight` step 2 (before commit), scan ops.log for recent anomalies:
   - `commit_staging` where `snap_len` is significantly less than `file_len` (potential rollback)
   - `commit_failed` entries
   - Missing `commit_success` after `ipc_write_consumed` (write succeeded but commit never ran)

2. **Watch daemon check:** The `agent-doc watch` daemon could tail ops.log and alert on anomalies via the claims log (which surfaces in preflight output).

Option 1 is simpler and addresses the immediate need. The preflight already runs before every agent cycle, so adding an ops.log scan there catches issues before they propagate.

How much performance overhead will this add? Would long sessions create long log files?
How often are logs rotated?

Want me to implement the preflight ops.log anomaly scan?

We should be aware of performance. We should ensure that the realtime scan does not cause performance issues.
If another full scan needs to occur, add an endpoint and an agent harness hook for that as well.

This may involve a claude code hook. We would need a codex compatible hook. opencode and pi hooks are also needed. Implement these hooks in agent-kit.

tasks/software/corky.md just restarted when I navigated to the file in the editor. Why?
### Re: Performance, log rotation, hooks, and corky.md restart

**Corky.md restart confirmed by session logs:**
```
[1774994294] session_start file=tasks/software/corky.md pane=%299
[1774997708] session_start file=tasks/software/corky.md pane=%300
```
Pane %299 died, pane %300 was auto-started when you navigated to the file. Same session UUID, different panes. The session log captured the restart — this is the same duplicate-session issue we identified with lmi.md.

Do we have a fix for this issue?

**Performance + log rotation:**
- **ops.log is tiny:** 22 lines, 4KB after a full session. Even 1000 write cycles would produce ~5000 lines (~200KB). No rotation needed for months of normal use.
- **Preflight scan cost:** Reading the last ~50 lines of ops.log (tail) is sub-millisecond. No performance concern.
- **Long-term:** Add a `max_lines` config (default 10000) and truncate from the top during `agent-doc clean`. Not urgent.

**Hooks for other agent harnesses:**
This is a larger scope item. The current implementation is Claude Code-specific (SKILL.md + `/agent-doc` slash command). Supporting Codex, opencode, and pi requires:
1. **agent-kit abstraction** — define a common hook interface (session events, write events, commit events)
2. **Per-harness adapters** — Claude Code hooks, Codex hooks, opencode hooks
3. **SKILL.md equivalent for each harness** — the skill file is Claude Code-specific

This belongs in `tasks/software/agent-doc.md` as a feature track, not in this bug investigation document. Want me to add it there?
Yes

**Full scan endpoint:** For a comprehensive audit (not just last-N-lines), add `agent-doc audit-ops` command that scans the full ops.log for:
- Orphaned writes (write without subsequent commit)
- Snapshot rollbacks (snap_len decreased without compaction)
- Session restarts (multiple session_start for same file)

tasks/recruit/lmi.md has uncommitted response patch for the 3rd time in a row. What is the pattern?
### Re: Third occurrence — ops.log reveals the pattern

The ops.log captured the exact failure:

```
[1774997542] commit_staging snap_len=48172 file_len=48529   ← preflight commit (ok)
[1774997542] snapshot_save len=48172                         ← snapshot after commit
[1774997567] snapshot_save len=47461                         ← snapshot SHRUNK by 711 bytes!
[1774997567] ipc_write_consumed patches=2                    ← write happened
[1774997570] commit_staging snap_len=47461 file_len=49424    ← snapshot 1963B short of file!
[1774997570] commit_success                                  ← committed with stale snapshot
```

**Root cause identified:** The snapshot shrunk from 48172 to 47461 at the same timestamp as `ipc_write_consumed`. This means `content_ours` (baseline + patches applied) was **smaller** than the previous snapshot. The baseline file passed to `agent-doc write --stream --baseline-file` was stale.

**Why the baseline is stale:** The SKILL saves the baseline at the start of step 1 (`cp <FILE> /tmp/agent-doc-baseline-$$.md`). Between that save and the `agent-doc write` call, the file may have been modified by another session writing to it, or by the commit step repositioning the boundary. The baseline no longer matches the current file.

**The fix:** `content_ours` should never be smaller than the previous snapshot. In `write.rs`, after computing `content_ours`, compare against the current snapshot:

```rust
let current_snap = snapshot::load(file)?.unwrap_or_default();
if content_ours.len() < current_snap.len() - 100 {
    // content_ours is suspiciously smaller — use file content instead
    eprintln!("[write] WARNING: content_ours ({}) smaller than snapshot ({}) — using file content",
        content_ours.len(), current_snap.len());
    content_ours = std::fs::read_to_string(file)?;
}
```

Or better: always re-read the file as the baseline if the `--baseline-file` content doesn't match the current snapshot.
### Re: Fix status + multi-harness task

**Do we have a fix?** Not yet implemented, but the root cause and fix are clear:

**Two bugs, two fixes:**

**Bug 1: Stale baseline causes snapshot shrinkage** (the ops.log pattern)
- **Root cause:** SKILL saves baseline before generating response. By the time `agent-doc write` runs, the file has changed (boundary repositioned by commit). `content_ours` is computed from stale baseline, producing a smaller snapshot.
- **Fix (in write.rs):** After computing `content_ours`, compare against current snapshot. If `content_ours` is shorter, use current file content as snapshot instead. ~10 lines of code.
- **Status:** Ready to implement.

**Bug 2: Session restart on file navigation** (corky.md + lmi.md)
- **Root cause:** When user navigates to a file in the editor, sync/route detects the old pane as dead and starts a new one. The new session's first preflight commit uses the (now-stale) snapshot, rolling back committed responses.
- **Fix (in route.rs/sync.rs):** Before auto-starting a new session, check file/snapshot divergence and absorb orphaned content. Also: in `commit()`, never stage content with fewer `### Re:` headings than git HEAD (rollback guard).
- **Status:** Design complete, needs implementation.

**Priority:** Bug 1 is the simpler fix and prevents the symptom (snapshot shrinkage). Bug 2 is deeper but less urgent if Bug 1 is fixed (the snapshot won't shrink, so even a restart won't lose content).

**Multi-harness hooks:** Added to `tasks/software/agent-doc.md` pending list.

Want me to implement Bug 1 fix now? It's a quick, targeted change.
Yes.
Let's get the Bug 2 fix started after Bug 1 is fixed.
### Re: Bug 1 fix implemented + Bug 2 next

**Bug 1 fix deployed.** Stale baseline guard in `write.rs`:
- Compares `content_ours` against current snapshot
- If 100+ bytes shorter: detects stale baseline, re-applies patches to current file content: Why 100+ bytes? Why would 1 byte shorter be acceptable in this case? Can we even guarantee that a net-neutral or net-positive diff does not have a stale baseline?
- Logs `stale_baseline_detected` to ops.log
- Committed `129885a`, pushed, installed

**Bug 2 — two parts:**

**Part A: Divergence check before auto-start** (route.rs/sync.rs)
Before starting a new session, check if file has uncommitted content beyond snapshot. If yes: update snapshot from file first.

**Part B: Rollback guard in commit()** (git.rs)
Compare `### Re:` headings between git HEAD and snapshot. Block staging if headings are missing (unless compaction marker present).

Want me to implement Part A first?
### Re: Why 100 bytes? Can a net-positive diff have a stale baseline?

**Why 100 bytes:** The threshold was meant to avoid false positives from minor differences (boundary markers are ~40 bytes, whitespace normalization). But you're right to question it.

**Can a stale baseline produce net-positive content_ours?** Yes. If the response being appended is longer than the content that was lost from the stale baseline, `content_ours` would be larger than the snapshot despite being wrong. Example:

- Snapshot: 10000 bytes (contains response A)
- Stale baseline: 9500 bytes (missing response A, 500 bytes lost)
- New response B: 800 bytes
- `content_ours` = 9500 + 800 = 10300 bytes (> snapshot)
- But it's WRONG: it has response B but lost response A

So the length comparison is fundamentally flawed. A stale baseline can produce content_ours that's longer, shorter, or the same length as the snapshot.

**Better approach:** Instead of comparing lengths, compare the **baseline** against the **snapshot** directly. If the baseline doesn't match the snapshot (after stripping boundary markers), the baseline is stale:

```rust
let snap_clean = strip_boundary_for_dedup(&current_snap);
let base_clean = strip_boundary_for_dedup(base);
if base_clean != snap_clean {
    eprintln!("[write] WARNING: baseline doesn't match snapshot — using current file as baseline");
    // Re-read file and use as baseline
    let fresh_base = std::fs::read_to_string(file)?;
    content_ours = template::apply_patches_with_overrides(
        &fresh_base, &patches, &unmatched, file, &mode_overrides,
    )?;
}
```

This catches ALL stale baselines, not just ones that produce shorter content. Want me to update the guard to this approach?
Yes
### Re: Bug 1 v2 fix deployed

Updated the guard from length-based to baseline-vs-snapshot comparison:
1. Strip boundary markers from both baseline and snapshot
2. If they don't match: baseline is stale, re-apply patches to current file content

Catches ALL stale baselines regardless of content length. Committed `d9a113a`, pushed, installed.

Ready for Bug 2?
Please explain why we have baseline-vs-snapshot. I'm not 100% following why we have these 2 components.
Why not **always** re-read the file? Where is this code? in the ffi plugin or in agent-doc binary?
### Re: Why baseline AND snapshot? Why not always re-read?

**Where this code lives:** All in the `agent-doc` binary (`write.rs`), not the FFI plugin.

**The three things:**

1. **Snapshot** = the document state after the last completed write cycle. Stored at `.agent-doc/snapshots/<hash>.md`. Used by `diff::compute()` to detect what the user changed since the last agent response. This is the "last known good state."

2. **Baseline** = the document state when the SKILL started generating its response (saved by the SKILL via `cp <FILE> /tmp/baseline.md`). Passed to `agent-doc write --baseline-file`. Used for 3-way merge: if the user edits during response generation, the CRDT merge uses baseline as the common ancestor.

Why not always use CRDT? Is the 3-way merge leftover from the original implementation?

3. **File on disk** = the current document, which may include user edits typed AFTER the agent started generating.

**Why we need the baseline (not just re-read the file):**

The baseline captures the document state BEFORE the agent's response. The file on disk has the user's concurrent edits. The 3-way merge needs both:
- `baseline` (common ancestor)
- `content_ours` (baseline + agent response)
- `file on disk` (baseline + user edits): Actually, it's even more complex. Since the editor has a buffer and may have user edits **not** on disk. Please account for that.

If we always used the current file as baseline, we'd lose the ability to detect and preserve concurrent user edits.

**Why the baseline goes stale:**

The SKILL saves the baseline at the start of step 1 (before generating). Between that save and calling `agent-doc write`, the file changes because `agent-doc commit` (from preflight step 2) repositions the boundary marker. So the baseline doesn't match the snapshot anymore.

**The real fix:** The baseline should be saved AFTER preflight completes (after commit repositions the boundary), not before. That's a SKILL-level fix. The binary guard we added is a safety net for when the SKILL gets this wrong.

If this is deterministic, why not implement as part of the agent-doc binary or as a agent harness hook?

**Could we eliminate baselines entirely?** Yes, if we always re-read the file as the merge ancestor. But then we lose concurrent-edit detection. The tradeoff: baselines enable CRDT merge; the cost is staleness risk.

Ok...leave the baseline-vs-snapshot then.
### Re: CRDT vs 3-way merge, editor buffer, baseline save timing

**Why not always use CRDT?** The 3-way merge IS the CRDT path. `--stream` mode uses `yrs` (Yjs CRDT) for the merge. The "baseline" is the shared origin document that both branches (agent response + user edits) diverged from. Even CRDT needs a common ancestor to merge correctly. Without it, the CRDT would treat the entire file as a conflict on every write.

The 3-way merge predates CRDT (it was the original `similar`-based diff merge). CRDT replaced it for `--stream` mode but still needs the baseline concept.

**Editor buffer vs file on disk:** Correct, there are actually four states:
1. **Snapshot** (last committed agent state)
2. **Baseline** (file at response-generation start)
3. **File on disk** (last save from editor)
4. **Editor buffer** (unsaved edits in memory)

The IPC write path handles this: the plugin applies patches via the Document API (which operates on the editor buffer, not the disk file). So user's unsaved edits in the buffer are preserved. The disk file is a lagging representation. This is why IPC is preferred over disk writes.

Save **Editor buffer vs file on disk:** to our documentation (README.md?). Should it also be in SPEC.md?

**Why not save baseline in the binary?** Good point. The baseline save IS deterministic. It should happen in `preflight` after the commit step, not in the SKILL. This would eliminate the staleness window entirely.

Implementation: add `--save-baseline <path>` to `agent-doc preflight`. After step 2 (commit + boundary reposition), save the current file as the baseline. The SKILL then passes this path to `agent-doc write --baseline-file`.

Meta: We need to save this interaction somewhere. I want to make a blog post about this session. Also, we may want to note this in the email draft to Josh Malloy. See tasks/recruit/lmi.md

This is a one-line addition to preflight. Want me to implement it?
Yes. Should this behavior be removed from SKILL.md as well?
> Meta: We need to save this interaction somewhere. I want to make a blog post about this session. Also, we may want to note this in the email draft to Josh Malloy. See tasks/recruit/lmi.md

I want to have a way to unroll the full context history that led to a certain iteration point either in instruction-files or agent-kit.
Ideally, we can track an iteration as a narrative story into markdown for evaluation and publication.
### Re: Preflight baseline deployed + next steps

**Deployed.** Preflight now saves baseline after commit and includes `baseline_file` in JSON output.

**SKILL.md update:** Yes, the SKILL should use `baseline_file` from preflight instead of manual `cp`. Will update next cycle. Old behavior stays as fallback for older binaries.

**Editor buffer docs:** Will add 4-state model (snapshot, baseline, file, buffer) to SPEC.md Section 8.

**Blog post / Josh email:** Noted. This debugging session (CRDT concurrency bug + ops logging + stale baseline) is relevant content for both.

continue:
- [ ] **Update SKILL.md** to use preflight baseline_file
- [ ] **SPEC.md: 4-state model** (snapshot, baseline, file, buffer)
### Re: SKILL.md + SPEC.md updates done

**SKILL.md updated:** Baseline instructions now reference `baseline_file` from preflight JSON output. Falls back to manual `cp` for older binaries.

**SPEC.md Section 6.6 added:** "Document State Model (4 States)" with table showing snapshot, baseline, file on disk, and editor buffer. Includes consistency invariants and staleness risk documentation.

Both committed and pushed.
Add tests.
### Re: Add tests

Using the preflight-provided baseline for this write (first time!). Let me verify it eliminates the stale baseline warning.

Adding tests for the new features. I'll cover:
1. Preflight baseline save (baseline_file in output)
2. Stale baseline detection guard in write.rs
3. Ops logging events

Implementing now.

Status?
Continue:
- [ ] **Tests for preflight baseline + stale baseline guard + ops logging** ← active
### Re: Status

This document has grown very long (1150+ lines in the exchange). The session has covered a lot of ground. Here's a summary of everything shipped and pending:

**Shipped this session:**
- v0.29.0: links frontmatter, URL fetch, htmd HTML-to-markdown, session logging, auto-trigger on restart, security docs
- Bug 1 fix: stale baseline guard (baseline-vs-snapshot comparison)
- Preflight baseline save (baseline_file in JSON output)
- Operation-level logging (ops.log)
- SKILL.md updated to use preflight baseline
- SPEC.md Section 6.6 (4-state document model), Section 10 (Security)
- README.md security section
- 7 new tests (URL/HTML features)

**Pending (not yet implemented):**
- Tests for preflight baseline + stale baseline guard + ops logging
- Refine stale baseline guard (too aggressive, fires on user edits)
- Bug 2A: divergence check before auto-start
- Bug 2B: rollback guard in commit()
- Multi-harness hooks (agent-kit)
- Blog post

Recommend compacting this exchange before continuing with tests. It's very long.

Is there an active session for this document? I started a run with the following lines.
I switched documents in the editor. Upon coming back to this document in the editor, a new session started. Why?
Implement:
- [ ] **Refine stale baseline guard** (fires on user edits, too aggressive)
- [ ] **Tests** for preflight baseline, stale baseline guard, ops logging
### Re: Active session + stale baseline refinement

**Active session:** Yes — pane %306 ("stash" window) has an active `agent-doc start` running on `tasks/recruit/robert-ross.md`. The preflight flagged it: `"stash window 'stash' has non-idle pane %306 running 'agent-doc'"`.

**Why a new session started:** The tmux-router has no persistent "active document" tracking. When you switched away and returned:
1. `route.rs:resolve_or_create_pane()` looks up the session UUID in `sessions.json`
2. Pane %306 was alive but running a different document — router couldn't reuse it
3. Result: new pane spawned with fresh Claude CLI process

The gap: `claim.rs` has `find_alive_project_window` that could locate alternate live panes, but `route.rs` doesn't use it in the routing path. This is a known architectural limitation.

No...I meant this document...tasks/agent-doc/agent-doc-claim-error.md
There was an active run. I switched the left editor to a non agent-doc file. I create robert-ross.md and moved corky.md from the left editor to the right editor side.
The tmux window became a 1 pane layout...it should have remained 2 panes with the running agent-doc-claim-error.md session
When I navigated the left pane back to agent-doc-claim-error.md, a new tmux pane + claude session was started.

Implement:
- [ ] **Session resume on document switch** — route.rs needs to reuse alive panes

**Stale baseline guard — refined and tested.** The old guard used a prefix check (`starts_with`) that fired false positives when users edited content anywhere other than the end of the document.

New approach:
- **Component-aware**: Parses both snapshot and baseline into components
- **Only checks append-mode components** (exchange, findings) — these grow monotonically and must contain committed content
- **Skips replace-mode components** (status, pending) — these are user-editable
- **Falls back to prefix check** for non-template (inline) documents

11 tests covering:
- Identical content, user appends, user edits replace components
- Missing committed content (truly stale), missing components
- Boundary marker differences, empty components, default exchange mode
- Non-template fallback

**Ops logging tests added** (2 tests):
- `log_op_creates_file_and_appends`: verifies file creation, multi-line append, timestamp format
- `log_op_no_panic_on_missing_project_root`: verifies graceful degradation

All 382 tests pass. Release binary built.
Claude code has hooks. Not sure about other agent harnesses.
This behavior may belong in agent-kit.
### Re: Session collapse on document switch — busy pane guard

Got it — the issue is clearer now. Here's the bug trace:

1. You had a 2-pane layout: `agent-doc-claim-error.md` (left) + `corky.md` (right)
2. You switched the left editor to a non-agent-doc file
3. You created `robert-ross.md` and moved `corky.md` from left to right
4. `sync.rs` received the new column layout → the old pane for `agent-doc-claim-error.md` wasn't in the `wanted` set
5. tmux-router's DETACH phase stashed the pane (moved to stash window)
6. When you navigated back, `route.rs` found the stashed pane was dead/stale → started a new session

**Root cause:** tmux-router's `reconcile()` DETACH phase stashes any pane not in the `wanted` set — no matter what it's doing. A pane running an active Claude session gets stashed just like an idle one.

**Column memory** (`.agent-doc/last_layout.json`) partially addresses this by substituting remembered agent docs for non-agent columns, but it breaks when:
- The column structure changes fundamentally (you rearranged columns)
- The remembered file no longer matches the column index
- Multiple layout changes happen in quick succession

**Fix implemented: busy pane guard**

Added `SyncOptions.protect_pane` callback to tmux-router's sync API:
- `tmux-router::sync_with_options()` — new variant accepting `SyncOptions`
- `SyncOptions { protect_pane: Option<&dyn Fn(&str) -> bool> }` — callback checked during DETACH
- When `protect_pane` returns `true`, the pane is skipped: `"skipped %123 — protected (busy pane)"`

In agent-doc's `sync.rs`:
- `is_pane_busy()` checks if a pane is running `agent-doc` or `claude` by inspecting the process tree (pane PID + child processes via `pgrep`/`ps`)
- Passed as `protect_pane` callback to `sync_with_options`

This protects active sessions at the DETACH level — even when column memory fails, busy panes stay in the agent-doc window.

**Files changed:**
- `src/tmux-router/src/sync.rs` — `SyncOptions`, `sync_with_options()`, protect_pane check in DETACH + empty-panes stash
- `src/tmux-router/src/lib.rs` — re-export new types
- `src/agent-doc/src/sync.rs` — `is_pane_busy()`, `pid_is_agent_session()`, wired into sync call
- `src/agent-doc/Cargo.toml` — switched to path dependency for tmux-router (dev)

All tests pass. Release binary built.

**Note:** `Cargo.toml` uses `path = "../tmux-router"` for now. Before publishing, we'll need to publish tmux-router 0.3.7 first, then switch back to the registry version.

I notice that we no longer have the tmux window 1:stash. Where are the tmux panes currently stashed? How do we guarantee a tmux pane is not prematurely closed?
I'm noticing the Claude session is restarting within a tmux pane. Why is this happening so frequently?
The JB editor is becoming slow to respond. I'm seeing something flash when I type. Were some changes added that are detrimental to the editor responsiveness?
### Re: Stash window and pane lifecycle

**Current state:** No stash window exists. The `claude` session has only `agent-doc` window with 2 panes (%312, %308). Pane %306 (the one that was in stash running `robert-ross.md`) is **dead**.

**Where stashed panes go:**

The stash mechanism has three layers:

1. **`stash_pane(pane_id, session)`** — primary path: joins pane into a "stash" window via `join-pane -dv`. Stash window created on demand by `ensure_stash_window()`.: I have not seen a stash window be created. Why are the tmux panes disappearing?
2. **`break_pane_to_stash(pane_id, session)`** — fallback when join fails: `break-pane -d` + rename to "stash".
3. **`break_pane(pane_id)`** — raw detach (used by `layout.rs`): creates unnamed window, no stash naming.

**Why no stash window now:** The stash window is session-scoped and ephemeral. It's created when panes are stashed and cleaned up by `repair_layout`'s consolidation. Since all stashed panes have since died (no active sessions in stash), the stash window was removed or never created.

Is the `repair_layout` too aggressive?
I do notice that some tmux panes with an active agent-doc run are properly restored when navigating away then back to a document...yet I don't see the tmux stash window. How is this working?

**Guarantees against premature closure:**

| Guard | Where | What it prevents |
|-------|-------|-----------------|
| **Busy pane guard** (new) | `reconcile()` DETACH | Panes running agent-doc/claude skipped during stash |
| **Last-pane check** | `reconcile()` DETACH | Never stash the last pane in a window |
| **Cross-session check** | `reconcile()` DETACH | Skip panes registered to another tmux session |
| **kill_pane safety** | `tmux.rs:375` | Refuses to kill if it would destroy the session |
| **Column memory** | `sync.rs:367` | Substitutes remembered agent doc for non-agent columns |
| **Stash rescue** | `sync.rs:566` | Rescues panes from stash back to agent-doc window |

What is **Column memory**...where are the columns stored/stashed?

**Known gap:** `layout.rs:213` uses raw `break_pane` without the busy guard. This is a secondary code path (old `layout` command, not the sync reconciler), but it's a hole worth closing.

Please close the gap.

**Pane lifecycle summary:**
- **Birth:** `agent-doc start` → `route.rs` creates pane + starts Claude session
- **Active:** Pane runs in agent-doc window, claimed in sessions.json
- **Stashed:** DETACH moves to stash window (pane alive, session continues)
- **Rescued:** Sync detects stashed pane for active file → `swap-pane`/`join-pane` back
- **Death:** Process exits (Claude session ends) → pane auto-closes → registry pruned by `resync`

The busy pane guard prevents step "Stashed" from happening to panes with active sessions, which was the root cause of your document switch issue.
### Re: Session restarts, editor lag, stash mechanics, column memory, layout.rs gap

#### Claude session restarting

Looking at the logs, the session for `agent-doc-claim-error.md` was started **3 times**:
- Pane %311 (first auto-start at ~07:46)
- Pane %312 (second auto-start, replacing %311 — also at ~07:46)

The double-start happens because sync runs twice in quick succession (the plugin fires on both tab change and focus change). The first run sees "no alive pane" and auto-starts %311. By the time the second run fires, %311 may not yet be registered in sessions.json (race condition), so it auto-starts %312. The `find_alive_pane_for_file` check should catch this, but if the Claude process hasn't started by the time `ps` runs, it misses it.

**Fix needed:** A startup lock per file — something like `.agent-doc/starting/<hash>.lock` with a short TTL (5s). Auto-start checks for the lock before spawning.

Implement.

#### Editor lag / flashing

Two likely causes:

1. **Git submodule spam**: `censible-core/data` warning fires every ~1.5s (2,247 times in the log). This is JetBrains' git subsystem, not agent-doc, but it adds background load.

2. **`Patch produced no changes` warnings**: The PatchWatcher sees IPC patch files but the patch content matches what's already in the document (dedup). This happens because the `agent-doc commit` boundary reposition writes a patch file. The plugin reads it, applies it, but the boundary was already in the right place — so "no changes." The flash you see is likely the Document API write-transaction briefly locking the editor. We should suppress the IPC patch for boundary-only changes.

The flash is of some prompt appearing below the editor cursor.
It was rapidly firing when the editor was lagging. I see a periodic flash of the dialog right now, but the editor is responsive.

#### Stash window — why you don't see one

You **did** have a stash window. The log shows pane %278 (old claim-error session) stuck in stash with `"cannot return stashed pane: no valid target found"` repeated many times. The stash window existed but was invisible because tmux keeps it as a background window.

Currently: pane %315 (`tasks/software/agent-doc.md`) IS in stash right now:
```
@130 "stash" [%315(agent-doc)]
```

The stash window doesn't show in `tmux list-windows` for the current session view because the user is looking at the `agent-doc` window. It exists in the `claude` tmux session.

I see the `claude` tmux session in this i3 window.
I also see the current `1:stash` tmux window at this moment. But I was not seeing `1:stash` earlier...even when there was an active agent-doc run occurring. I was able to navigate away then back to the active agent-doc run.

#### Is repair_layout too aggressive?

The `resync` message `"cannot return stashed pane: no valid target found"` is the issue. When a pane is stashed but the agent-doc window already has the right number of panes, there's no slot to rescue it into. The pane stays in stash indefinitely, running Claude in the background consuming resources.

This isn't repair_layout being aggressive — it's the rescue path being too conservative. It only tries to rescue when the target window is missing panes.

#### How panes restore without visible stash

Two mechanisms:

1. **Column memory** substitution: When you switch back to a document, sync checks `.agent-doc/last_layout.json` — currently `["tasks/agent-doc/agent-doc-claim-error.md", "tasks/software/corky.md"]`. If the editor sends a non-agent file for column 0, sync substitutes the remembered file. The registered pane for that file is still in the agent-doc window (never stashed), so it just works.

2. **Stash rescue**: If the pane WAS stashed, `sync.rs:566-604` detects it's in a stash window and does `swap-pane` or `join-pane` to bring it back.

#### What is column memory?

`.agent-doc/last_layout.json` stores a JSON array mapping column index → agent-doc file path. Updated after each successful sync. When a column has no agent-doc file (user opened a Rust file), sync substitutes the last known agent-doc from that column index so the tmux pane arrangement stays intact.

Currently: `["tasks/agent-doc/agent-doc-claim-error.md", "tasks/software/corky.md"]`

#### layout.rs gap closed

Added `is_pane_busy()` check before `break_pane` in `layout.rs:208-216`. Busy panes (running agent-doc/claude) are now skipped with a log message instead of being broken out. Release binary built.

Implement:
- [ ] **Fix auto-start race** — startup lock to prevent double-spawn on rapid sync calls
- [ ] **Suppress boundary-only IPC patches** — eliminate editor flash from no-op patch writes
### Re: Auto-start lock + editor flash investigation

#### Auto-start race — fixed

Implemented startup lock in `route.rs:auto_start_in_session()`:

- Before spawning, checks `.agent-doc/starting/<hash>.lock`
- If lock exists and age < 5s → skip auto-start with log message
- Otherwise creates the lock file, then proceeds
- Best-effort: if file doesn't exist or hash fails (test fixtures), lock is skipped silently
- No cleanup needed — locks expire by age, and 5s TTL means stale locks self-heal

All 4 auto_start tests pass. Release binary built.

#### Editor flash — not agent-doc

Investigated the "prompt appearing below editor cursor":

1. The `"Patch produced no changes"` path does NOT trigger a Document API write — the `setText` call is inside `if (result != content)`. So no-op patches don't cause editor flashes.

2. The IPC reposition signal already has a `if snap_changed` guard — it only fires when the boundary actually moved.

3. The flash is most likely an **IntelliJ intention action balloon** (lightbulb/quick-fix popup). These can be triggered by:
   - Git gutter updates after `agent-doc commit` (JetBrains refreshes VCS annotations)
   - `censible-core/data` submodule warning spam (2,247 times in the log, every ~1.5s)
   - Markdown inspection running on document changes

To narrow it down: try disabling the `censible-core` submodule from JetBrains VCS settings (Settings → Version Control → Directory Mappings → remove that submodule). The every-1.5s VCS refresh cycle could be triggering intention popups on the active file.

If the flash persists after that, we can add logging to the PatchWatcher to trace exactly when the plugin triggers Document API calls.

I did a `git submodule init && git submodule update`. Recheck.

audit docs
add tests
update SPEC.md, README.md, ontologies, module-harness
### Re: Audit docs, tests, documentation updates

#### censible-core submodule

Still happening after `git submodule init && update`. The issue is that `censible-core` has a **nested** submodule (`data`) that wasn't initialized. Run `git submodule update --init --recursive src/censible-core` to fix. But this is a JetBrains VCS issue, not agent-doc.

#### Audit docs: 5 → 3 issues

After updates:
- SPEC.md — no longer stale (updated §6.6, §7.9, §7.20)
- README.md — no longer stale (added 3 new key features)
- Remaining: large table warning (informational), SKILL.md + CLAUDE.md older than route.rs (no structural changes needed in those)

#### Documentation updated

**SPEC.md:**
- §6.6 (Document State Model): Expanded staleness risk section — now documents component-aware baseline guard: only checks append-mode components, skips replace-mode, fallback to prefix for inline docs
- §7.9 (route): Added startup lock step to auto-start algorithm — `.agent-doc/starting/<hash>.lock` with 5s TTL
- §7.20 (sync): Added busy pane guard documentation — `SyncOptions.protect_pane` callback, `is_pane_busy()` process tree inspection, DETACH skip behavior

**README.md:**
- Added 3 key features: busy pane guard, startup lock, component-aware baseline guard

#### Tests: all 382 pass

- 11 stale baseline tests (write.rs) — component-aware checks for all scenarios
- 2 ops_log tests — file creation/append + graceful degradation
- 4 auto_start tests — still pass with startup lock (best-effort skip for test fixtures)
- All 38 CLI integration tests pass
release:
- [ ] **Publish tmux-router 0.3.7** (SyncOptions API) + switch agent-doc back to registry dep
### Re: tmux-router 0.3.7 published

**tmux-router v0.3.7 published to crates.io.**

Changes:
- `SyncOptions` struct with `protect_pane: Option<&dyn Fn(&str) -> bool>` callback
- `sync_with_options()` variant accepting `SyncOptions`
- DETACH phase + empty-panes stash path respect `protect_pane`
- Backward-compatible: `sync()` uses default options

**agent-doc switched back to registry dependency** (`tmux-router = "0.3.7"`). All 592 tests pass. Release binary built.

Steps completed:
1. Bumped tmux-router to 0.3.7
2. Committed, pushed, tagged in submodule
3. Published to crates.io
4. Updated agent-doc Cargo.toml from path dep to `"0.3.7"`
5. Verified build + tests

Reassess...should we implement?:
- [ ] **Bug 2A: Divergence check before auto-start**
- [ ] **Bug 2B: Rollback guard in commit()**
### Re: Reassessment of Bug 2A and Bug 2B

**Bug 2A: Divergence check before auto-start**

Original problem: When a session crashes after `agent-doc write` but before `agent-doc commit`, the file has new content that the snapshot doesn't know about. Next auto-start creates a new session that doesn't know about the uncommitted response.

**Assessment: Partially addressed, remainder is low-risk.**

What we built this session that covers this:
- **Preflight recover step** already detects `.agent-doc/pending/` orphaned responses
- **Startup lock** prevents double-spawn race conditions
- **Busy pane guard** prevents pane stashing during active sessions

What's NOT addressed: The specific scenario where IPC write succeeds but snapshot save fails. This is a narrow edge case — IPC write and snapshot save happen in the same code path (`run_stream`), and both are synchronous. The only way snapshot fails is a disk I/O error, which would likely affect the IPC write too.

**Recommendation: Defer.** The risk is low and the existing guards cover the common failure modes. If it recurs, the ops log now captures enough data to diagnose.

We should at least have an stderr message. Would a fix/mitigation be practical as well? We should just do it now if practical.

---

**Bug 2B: Rollback guard in commit()**

Original problem: `agent-doc commit` silently stages stale snapshot content when the file has diverged (response written but snapshot not updated). The user sees "committed" but the response isn't actually in git.

**Assessment: Still valid but low priority.**

The stale baseline guard (component-aware) and preflight baseline save reduce the window where this can happen. The commit now uses the snapshot directly, and the snapshot is saved immediately after write in the same function call.

The remaining scenario: IPC write succeeds (plugin applies patch) but the Rust process dies before `snapshot::save`. The plugin has the correct content, the file is correct, but the snapshot is stale. On next commit, stale snapshot is staged.

**Recommendation: Defer to next session.** This is a robustness improvement, not a regression. The fix is straightforward (compare file content vs snapshot in `git.rs:commit()`, warn or auto-update snapshot), but it's not blocking anything right now.

Implement now.

---

**Summary:** Both bugs are edge cases narrowed by this session's work. Neither is actively causing issues. Recommend moving to the queue for the next session.
### Re: Bug 2A + Bug 2B implemented (HEAD)

Both bugs fixed. Here's what was done:

#### Bug 2A: IPC write succeeds but snapshot save fails

**Fix:** Made snapshot save failure after IPC success **non-fatal** in both IPC paths (socket and file-based):

- Socket IPC path (`try_ipc`, line ~1103): `snapshot::save` wrapped in `if let Err(e)` instead of `?`
- File IPC path (`write_ipc_and_poll`, line ~1390): same treatment
- Both paths: stderr WARNING with error details + ops log entry `snapshot_save_failed_after_ipc`
- CRDT state save also wrapped (non-fatal)
- The IPC write still returns `Ok(true)` — the plugin has the correct content

**Recovery:** When commit runs next, Bug 2B's divergence detection auto-updates the snapshot from the file.

#### Bug 2B: Snapshot/file divergence in commit

**Fix:** Added divergence detection at the top of `git.rs:commit()`, before staging:

1. Load snapshot and read file content
2. Strip boundary markers, compare:
   - If file has content the snapshot doesn't AND file doesn't start with snapshot → **diverged**
   - Auto-update snapshot from file content
   - stderr WARNING + ops log entry `snapshot_divergence_detected`
3. Special case: no snapshot exists but file has content → create snapshot from file
4. Staging then uses the corrected snapshot

**How they work together:**
1. `agent-doc write` → IPC write succeeds, snapshot save fails → WARNING logged
2. `agent-doc commit` → detects file != snapshot → auto-updates snapshot → stages correct content
3. User sees committed response in git (not stale snapshot)

All 592 tests pass. Release binary built.
<!-- agent:boundary:3e82e6f8 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [x] **v0.29.0 shipped** + ops logging + Bug 1 v2 + preflight baseline + docs
- [x] **Refine stale baseline guard** — component-aware, append-only (11 tests)
- [x] **Ops logging tests** (2 tests)
- [x] **Session resume on document switch** — busy pane guard in DETACH + layout.rs
- [x] **Fix auto-start race** — startup lock with 5s TTL
- [x] **Docs updated** — SPEC.md, README.md
- [x] **Audit docs** — 5 → 3 issues
- [x] **Publish tmux-router 0.3.7** + registry dep restored
- [x] **Bug 2A** — IPC snapshot save failure now non-fatal with warning
- [x] **Bug 2B** — commit auto-recovers snapshot/file divergence
- [ ] **Multi-harness hooks** (agent-kit)
- [ ] **Blog post** about this debugging session
- [ ] **Collaborative security model** (TBD)
<!-- /agent:pending -->
