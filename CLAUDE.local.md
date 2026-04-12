# agent-loop — Local

Dog-fooding the agent-doc CLI tool and `/agent-doc` Claude Code skill via plan.md sessions.

## agent-doc Release Checklist

When releasing agent-doc, always run `cargo build --release` inside the submodule after publishing. The `.bin/agent-doc` symlink points to `src/agent-doc/target/release/agent-doc` — without a release build, the symlink target stays stale.

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
