# agent-loop — Local

Dog-fooding the agent-doc CLI tool and `/agent-doc` Claude Code skill via plan.md sessions.

## agent-doc Release Checklist

When releasing agent-doc, always run `cargo build --release` inside the submodule after publishing. The `.bin/agent-doc` symlink points to `src/agent-doc/target/release/agent-doc` — without a release build, the symlink target stays stale.

## Browser Automation

Prefer `chromium-bridge` CLI over Chrome DevTools MCP tools for browser automation. See [runbooks/browser-automation.md](runbooks/browser-automation.md).

## Audio Transcription

Prefer `corky transcribe` for audio transcription over raw whisper CLI. Use with `--diarize` for speaker diarization and `--speakers "Name1,Name2"` for labeling. GPU acceleration is used automatically when available.

## WordPress Migration Patterns (monsterrodholders)

### Page Option Migrations

When migrating WooCommerce pages (cart, my-account) or site settings (homepage), follow this pattern:

1. **Flip the option** (atomic update):
   ```bash
   wp option update woocommerce_cart_page_id <new_id>
   wp option update woocommerce_myaccount_page_id <new_id>
   wp option update page_on_front <new_id>
   ```

2. **Add 301 redirects** (preserve SEO equity from live site):
   - Cloudflare rules: `/old-page-2/` → `/new-page/` (if direct access available)
   - Or `.htaccess` / WordPress Redirect plugin
   - Test: `curl -I https://monsterrodholders.com/old-page-2/` should return 301

3. **Delete old pages** (after redirect verification):
   - Option 1: Delete immediately after testing
   - Option 2: Mark as draft, wait 48h for SEO bake time, then delete

4. **Search-replace old URLs** in content:
   ```bash
   wp search-replace '/old-page-2/' '/new-page/' --skip-columns=guid
   ```

5. **Flush caches**:
   ```bash
   wp cache flush
   wp cloudflare flush  # if WP Cloudflare plugin installed
   ```

### Stripe Secrets Hygiene

**Never dump full Stripe settings to stdout** — livestreams expose the entire `woocommerce_stripe_settings` blob including secret keys.

**Safe operations:**
- Use `wp option pluck <option> <field>` for single-field reads (smaller, safer output)
- Filter with `jq`: `wp option get woocommerce_stripe_settings | jq 'del(.secret_key, .webhook_secret)'`
- For updates: use `wp option patch update` (field-targeted) or pipe via `passage` for secrets
- **If livestream VOD contains exposed JSON:** trim the segment during post-production

**Storage:**
- Keep Stripe keys in `passage` at `btak/monsterrodholders/{stripe,paypal}/*`
- Reference during imports/exports without printing

<!-- tsift:code-navigation v=0.1.62 -->
## Code Navigation

Run `tsift status` at session start from the owning repo root. If the task or file lives under a git submodule (for example `src/tsift/...`), switch to that submodule root first so the harness loads the narrower local instructions and repo state instead of the superproject root. If status prints a `run:` recommendation for stale or missing tsift state, run `tsift status --fix` before relying on tsift results; when the harness cannot perform write commands, ask the user to run the printed command instead. Codex projects can install a prompt-time auto-reindex hook with `tsift init --codex`.

Use the commands listed in its `use:` output:
- `tsift --envelope search <query> --budget normal` — AST-aware hybrid search preview (prefer over grep/rg)
- `tsift --envelope explain <symbol> --budget normal` — callers, callees, community preview
- `tsift graph <symbol> --callers` / `--callees` — call graph navigation
- `tsift summarize <symbol>` — cached summary (only when listed in `use:`)
- `tsift workflow search` — ordered exact/search/explain/summarize/digest recipe that preserves result handles across expansions

When a search envelope includes `report.scale_guard`, run one of its `narrow_commands` before dispatching parallel agents. The guard means the original result set or corpus is broad enough that fan-out should start from a narrower cited handle, path, or exact query.

Prefer bounded digest commands over raw transcript, diff, and verbose-log reads:
- `tsift --envelope session-review <path> --next-context --budget normal` or `tsift --envelope context-pack <path> --budget normal` instead of replaying long session docs, JSONL transcripts, or agent-doc runtime logs with `cat`, `tail`, or `sed`.
- `tsift diff-digest [path]` (`--cached`, `--revision <rev>`) instead of `git diff`, `git show`, or patch-style `git log`.
- `tsift --envelope __digest-runner --kind test --path . --shell-command '<test command>'` / `tsift --envelope __digest-runner --kind log --path . --shell-command '<build command>'` for noisy test/build/install output, or let the rewrite/hooks create those artifact-backed envelopes for `cargo test`, `pytest`, and verbose cargo commands.
- If RTK is installed, digest-runner delegates supported generic command families through `rtk rewrite` and records the chosen compact filter in `report.filter` while preserving tsift artifact handles.
- If your harness does not support Claude-style `PreToolUse` hooks, run `tsift rewrite --run '<command>'` to execute the same envelope-first, artifact-backed tsift equivalent manually.

For local verification, run `make check` before committing. After local changes, check the latest GitHub Actions CI run with `gh run list --workflow CI --limit 1` and fix any failing tests before calling the work complete.

Only read full source files when tsift results are insufficient.
<!-- /tsift:code-navigation -->

