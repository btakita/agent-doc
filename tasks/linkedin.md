---
session: a953811a-bf72-4aa9-bbd6-5904a5a07a1d
resume: null
agent: null
model: null
branch: null
---
### LinkedIn

**About section** updated (copy-paste ready):
> Over 20 years of software development experience... Most recently built the full real-time AI avatar pipeline at Presence AI... Now building spec-driven agent tooling and collaborative intelligence workflows in Rust.

**Post draft**: `mail/social/20260224-agent-tooling-linkedin.md` (status: draft)
- Self-referential dog-fooding line added: "Even this post — drafted in a corky social markdown file and published via `corky social publish`."
- To publish: change `status: draft` → `status: ready`, then `corky social publish <file>`
- Requires LinkedIn OAuth setup first (Company Page + developer app)

**Company taglines** (saved to `mail/topics/brian-takita/README.md`):
- Brand identity: "Collaborative intelligence tooling — from spec to ship."
- Marketing/pitch: "Spec-driven agent tooling for teams that ship."

### Corky v0.11.0 Features

**Social media posting** (`corky social`):
- 8 source files: platform enum, profiles.toml, YAML drafts, token store, OAuth2, LinkedIn API, publish, CLI
- 54 edge-case tests (P1-P9, D1-D9, T1-T9, A1-A5, PB1-PB10)
- SPECS.md §12 with full edge case table
- CLI: `auth`, `draft`, `publish`, `check`, `list`, `rename-author`

**Scheduling** (`corky schedule`):
- `schedule.rs` — scan social/ and drafts/ for items where `scheduled_at <= now`, dispatch to existing publish functions
- `ScheduledKind::Social` / `ScheduledKind::Email` — no shared trait, just a match
- 12 unit tests (S1-S10 + 2)
- CLI: `schedule run [--dry-run]`, `schedule list`
- Integrated into `corky watch` loop (IMAP sync + scheduled publishing in one daemon)

**Email draft YAML migration** (`corky draft migrate`):
- `EmailDraftMeta` serde struct with YAML frontmatter (snake_case, matching social drafts)
- Dual-parser: detects `---` at line 1 → YAML path, else legacy `**Key**: value`
- Migration command: `corky draft migrate [--dry-run]`
- 17 new tests across draft/mod.rs, draft/new.rs, draft/migrate.rs, schedule.rs

**Topics** (`corky topics`):
- Topic config in `.corky.toml` with `keywords`, `mailboxes`, `description`, `contacts`
- CLI: `topics list`, `topics suggest`

**Bidirectional topic sync** (PR [#9](https://github.com/btakita/corky/pull/9), squash merged):
- `sync_topics()` in `mailbox/sync.rs` — mtime-based bidirectional file sync
- `topics_for_mailbox()` in `config/topic.rs` — filter by mailbox name
- Forward: root `topics/{name}/` → `mailboxes/{mb}/topics/{name}/`
- Reverse: mailbox → root (if mailbox file is newer)
- 12 new tests, SPECS.md §7.7 Topic Sync (edge cases TS1-TS8)

**Release**: v0.11.0 published to GitHub, crates.io, PyPI (all with updated docs)

### Decisions

- **Directory naming**: Keep `mail/` — alternatives (`mailbox/`, `comms/`, `correspondence/`) rejected. Documented in SPECS.md §2.5.
- **Scheduling architecture**: No shared trait — thin `schedule.rs` orchestrator dispatches to existing publish paths via `ScheduledKind` match.
- **LinkedIn API**: No native scheduling support — client-side scheduling via `corky schedule run` / `corky watch`.
- **Topic sharing**: Configured via `.corky.toml` `mailboxes` field, not git submodules. Bidirectional mtime-based sync.
- **Brand topic**: `mail/topics/brian-takita/README.md` as canonical shareable context (not just auto memory).

## User

Let's go back to the LinkedIn post. Should we include corky topics, or would this post become too complicated?

## Assistant

**Skip it.** The post is already at the sweet spot (~250 words) and has a clean narrative arc: two tools → methodology → dog-fooding payoff. Adding topics would introduce a third concept that needs explanation (what are topics? how do they sync? why bidirectional?) and break that flow.

"Topics" is already mentioned in passing — line 17 says "syncs topics, contacts, and threads." That's enough for a first post. Anyone curious will click the repo link.

**Better approach:** topics is its own post. The bidirectional mailbox sync story is genuinely interesting — configuring what a collaborator sees via `.corky.toml`, mtime-based conflict resolution, the "topic follows conversation" concept. That's a standalone narrative worth its own 200-word post, framed as something like "How I share AI context with collaborators without giving them my whole inbox."

**Content cadence:**
1. **Post #1** (current) — Introduction to corky + agent-doc + spec-driven methodology. Ship as-is.
2. **Post #2** — Deep dive on topics/mailbox architecture. More technical, targets the "how does it actually work" audience.

Post #1 plants the flag, post #2 shows depth.

## User

We should mention tmux routing. I also have a screenshot at /home/brian/Pictures/agent-loop/2026-02-25_18-22-my-humble-setup.png

## Assistant

The screenshot is perfect — multiple tmux panes each running a different agent-doc session, editors alongside terminals. That's the "agent loop" made visible.

**Attach it as the post image.** LinkedIn posts with images get significantly more engagement, and this one visually proves the dog-fooding claim.

For the text, weave tmux routing into the existing agent-doc paragraph rather than adding a new section:

**Current:**
> agent-doc — a document-as-interface for persistent AI conversations. Edit a markdown file offline, press a hotkey, and the tool diffs your changes, sends them to an AI agent, and writes the response back inline. Git tracks every exchange. I use it as my primary planning and iteration tool — including for writing this post.

**Proposed:**
> agent-doc — a document-as-interface for persistent AI conversations. Edit a markdown file, press a hotkey, and the tool diffs your changes, routes the submit to the correct tmux pane, and writes the AI response back inline. I run multiple sessions in parallel — one per task document — each a coherent, persistent conversation tracked by git. That screenshot is my actual setup.

Changes:
- "routes the submit to the correct tmux pane" — explains routing without jargon
- Merges the "multiple sessions in parallel" content (currently a standalone paragraph) into the agent-doc block
- "That screenshot is my actual setup" — ties image to text, reinforces dog-fooding
- Frees the standalone paragraph (lines 23-24) to be cut or shortened

Post stays ~250 words. Want me to update the draft file?

## User

Yes update the draft. replaced ~right tmux pane~ with "correct tmux pane"

## Assistant

Updated `mail/social/20260224-agent-tooling-linkedin.md`. Changes:

1. **agent-doc paragraph** — rewrote with tmux routing and parallel sessions, using "correct tmux pane"
2. **Standalone paragraph** (was "With agent-doc, I run multiple...") — trimmed to just the compaction sentence ("When a session gets long...")
3. **"That screenshot is my actual setup"** — ties the attached image to the text

Remember to attach `~/Pictures/agent-loop/2026-02-25_18-22-my-humble-setup.png` when posting.

## User

Walk me through finishing the custom app. I left off with creating the company page

## Assistant

Picking up from Company Page created:

**Step 1: Create the App**
1. Go to [linkedin.com/developers/apps](https://www.linkedin.com/developers/apps)
2. Click "Create app"
3. Fill in: App name ("corky"), LinkedIn Page (select your new page), Privacy policy URL (repo URL works), App logo (optional)
4. Accept terms, click "Create app"

**Step 2: Request Products**
1. Go to the **Products** tab on your app page
2. Request: **Share on LinkedIn** (`w_member_social`) and **Sign In with LinkedIn using OpenID Connect** (`openid`, `profile`, `email`)
3. Both approve instantly for personal use

**Step 3: Configure OAuth redirect**
1. Go to the **Auth** tab
2. Add redirect URL: `http://127.0.0.1:8484/callback`
3. Copy your **Client ID** and **Client Secret**

**Step 4: Configure corky**

```bash
export CORKY_LINKEDIN_CLIENT_ID="your-client-id"
export CORKY_LINKEDIN_CLIENT_SECRET="your-client-secret"
```

Or in `mail/.corky.toml`:
```toml
[social.linkedin]
client_id = "your-client-id"
client_secret = "your-client-secret"
```

**Step 5: Authenticate**
```bash
corky social auth linkedin --profile btakita
```
Opens browser → approve → token saved to `~/.config/corky/tokens.json`.

**Step 6: Publish**
```bash
sed -i 's/status: draft/status: ready/' mail/social/20260224-agent-tooling-linkedin.md
corky social publish mail/social/20260224-agent-tooling-linkedin.md
```

**Caveat**: LinkedIn Posts API does not support image attachments on organic (personal) posts. For text-only, the API works. To include the screenshot, publish manually via LinkedIn's web composer instead (copy-paste the post body + attach the image).

Is there a different type of app that I can make that supports images on posts?

## User

Can you help me to create a logo for the app?

## Assistant

**Correction on images**: My earlier caveat was wrong. The LinkedIn API **does** support image attachments on personal posts. The flow:

1. `POST /rest/images?action=initializeUpload` with `owner: urn:li:person:{id}` — returns upload URL + image URN
2. `PUT` the binary image to the upload URL
3. `POST /rest/posts` with `content.media.id` set to the image URN

Same `w_member_social` scope, same app. No different app type needed. Corky's `linkedin.rs` would need image upload support added — that's a future enhancement. For now, publish manually with the screenshot or text-only via API.

**On the logo**: I can't generate images, but options to get one quickly:

1. **Text mark** — "corky" in a monospace font with a distinctive color. Fastest, works at any size, zero design effort. Good enough for a developer app.
2. **AI image generation** — DALL-E, Midjourney, etc. Prompt: "Minimalist cork/bottle cap icon, flat design, developer tool aesthetic, warm amber tones, transparent background"
3. **SVG by hand** — Inkscape or text editor. Simple geometric cork shape.
4. **favicon.io** — turn text or an emoji into a proper icon set

For the LinkedIn developer app, any square image works. Option 1 gets you unblocked fastest.

## User

Add image support to corky

## Assistant

**Plan for LinkedIn image support:**

| File | Change |
|---|---|
| `social/draft.rs` | Add `image: Option<String>` to `SocialDraftMeta` — relative path to image file |
| `social/linkedin.rs` | Add `upload_image(token, author_urn, image_bytes) -> Result<String>` returning image URN. Modify `create_post()` to accept `Option<String>` image URN — set `shareMediaCategory: "IMAGE"` + `media` field when present |
| `social/publish.rs` | After token lookup: if `draft.meta.image` set, read file → `upload_image()` → pass URN to `create_post()` |
| SPECS.md | Update §12 with image support + edge cases |

**Draft format:**
```yaml
---
platform: linkedin
author: btakita
image: assets/screenshot.png
status: ready
---
```

**API upgrade note:** Current `linkedin.rs` uses v2 UGC API (`/v2/ugcPosts`). LinkedIn is deprecating this. Recommend upgrading to REST API (`/rest/posts` + `/rest/images`) as part of this change — simpler image flow, no `serviceRelationships`/`recipes`.

**Edge cases:** IM1 (no image → text-only), IM2 (file not found), IM3 (too large), IM4 (unsupported format), IM5 (upload succeeds/post fails), IM6 (relative path resolution from draft file location).

Want me to implement? This should be a PR on a branch since it modifies the LinkedIn API integration.

## User

Can we support inline images in addition to the post image?

## Assistant

**LinkedIn doesn't support inline images in post text.** The body is plain text — no markdown, no HTML, no embedded images.

What LinkedIn *does* support:
- **Single image** — one attached image (our `image:` field)
- **Multi-image carousel** — up to 20 images via `content.multiImage.images[]`

**Recommendation: use `images:` (list) from the start** instead of `image:` (single). This handles both cases cleanly:

```yaml
---
platform: linkedin
author: btakita
images:
  - assets/screenshot.png
status: ready
---
```

Publish logic: `len(images)` → 0: text-only, 1: single image (`content.media`), 2+: carousel (`content.multiImage`).

If by "inline" you meant markdown `![alt](path)` in the body getting extracted and uploaded — that's also possible as a convenience (parse body for image refs, upload them, add to carousel). But it still maps to the same API. More complexity, same visual result.

**Updated plan:** Use `images: Vec<String>` instead of `image: Option<String>`. Everything else stays the same. Want me to implement?

## User

Yes implement in a PR branch, patch or minor bump?

## Assistant

**Minor bump → 0.12.0.** This adds new functionality (image uploads + LinkedIn REST API migration), not a patch fix.

**Branch:** `20260225/linkedin-images`

**Implementation plan:**
1. `social/draft.rs` — add `images: Vec<String>` with `#[serde(default, skip_serializing_if = "Vec::is_empty")]`
2. `social/linkedin.rs` — add `upload_image()`, upgrade `create_post()` from v2 UGC to REST API (`/rest/posts`), accept optional image URNs for single image or carousel
3. `social/publish.rs` — resolve image paths relative to draft file, upload each, pass URNs to `create_post()`
4. Tests — IM1-IM6 edge cases + draft round-trip with images
5. SPECS.md §12 — update with image support + edge case table

Implementing now.

## User

Is the LinkedIn app that I am adding meant to only be used by me...or meant to be used by anyone using Corky for publishing to LinkedIn?

## Assistant

**Just you, right now.** The LinkedIn developer app you're creating is tied to your personal LinkedIn Company Page and your OAuth credentials. It's a "Development" app — LinkedIn doesn't distinguish between "personal use" and "multi-user" apps at the creation level. The difference is in how credentials are distributed.

**How it works today (your app):**
- You create the app → you get `client_id` + `client_secret`
- You put those in your `.corky.toml` or env vars
- `corky social auth linkedin` opens *your* browser → *you* approve → token stored for *your* URN
- Only you can publish with that token

**If someone else wants to use corky to publish to LinkedIn:**
They have two options:

1. **Create their own app** (recommended for now) — They go through the same steps: create their own Company Page, register their own developer app, get their own `client_id`/`client_secret`, configure their `.corky.toml`. Corky already supports this — each user has their own config and token store.

2. **Use your app with multi-user OAuth** (future) — You'd "verify" your app with LinkedIn (requires review/approval), then anyone could authorize through your app. They'd run `corky social auth linkedin`, it would redirect to LinkedIn's consent page using *your* `client_id`, they'd approve, and the token would be stored under *their* URN. This is how "real" apps work (like Buffer, Hootsuite). Corky's architecture already supports this (tokens are URN-keyed), but it requires LinkedIn to approve your app for broader access.

**For now: option 1.** Your app is just for you. Each corky user creates their own. This is the simplest path and requires no LinkedIn review process. The multi-user app path is a future concern — only matters if corky gets adoption and you want to offer a hosted OAuth flow.

## User

Should it be corky/assets/logo.svg or a different name?
Also, I need to have a png or webm for uploads. Should those assets be built from the svg and gitignored...or committed to the repo?

## Assistant

**`assets/logo.svg`** — yes, that's the right name and location. Standard convention for Rust projects.

**Commit the PNG, don't build it.** Reasons:

1. **SVG → PNG conversion requires external tools** (Inkscape, rsvg-convert, ImageMagick). Adding a build step for a single static file creates a dependency that every contributor/CI needs. Not worth it for one logo.
2. **The PNG won't change often.** Logos are near-static. The binary diff in git is negligible for a single small image.
3. **Precedent**: Most Rust projects commit their logo PNGs directly.

**Recommended structure:**

```
corky/assets/
  logo.svg        # source of truth (committed)
  logo-128.png    # 128×128 for LinkedIn app, README, etc. (committed)
  logo-64.png     # 64×64 for favicon/small contexts (committed, optional)
```

**Don't gitignore the PNGs.** Re-export manually when the SVG changes. Optional Makefile target if you want it later:

```makefile
assets/logo-128.png: assets/logo.svg
	rsvg-convert -w 128 -h 128 $< -o $@
```

For the LinkedIn developer app, you need a square image ≥ 100×100 pixels. `logo-128.png` covers that. Upload it on the app's **Settings** page under "App logo."

## User

ok...do it

Does agent-doc need to update its skills?

## Assistant

**Done.** Exported PNGs from the existing `assets/logo.svg`:

- `assets/logo-128.png` (10KB) — use this for the LinkedIn developer app logo
- `assets/logo-64.png` (4.5KB) — for favicon/small contexts

The SVG is a corgi-in-a-bottle design with warm amber tones. Renders cleanly at both sizes.

**Next step:** Upload `logo-128.png` to your LinkedIn developer app under **Settings → App logo**.

**On agent-doc skills:** No — the agent-doc skill (`SKILL.md`) defines the document session workflow (diff, respond, write back). It doesn't need updating for corky feature work. The skill auto-update check (`agent-doc --version` vs frontmatter) runs on every submit already. The skill would only need an update if the `agent-doc` binary itself gets a new version with a new `agent-doc skill install`.

## User

Read https://www.linkedin.com/legal/l/api-terms-of-use Anything I should be worried about? Any highlights to point out which are relevant to me?

## Assistant

**One yellow flag.** Section 3.1 says developers must not:

> "Employ APIs to automate posting on the LinkedIn Services"

This directly describes what `corky social publish` does. However:

- LinkedIn's **Share on LinkedIn** product (`w_member_social`) exists *specifically* for apps to create posts. That's its purpose.
- The restriction likely targets bot farms and spam tools — not user-initiated single-post publishing.
- Buffer, Hootsuite, Sprout Social all use this exact API. LinkedIn approves them.
- Key distinction: **user-initiated** (`corky social publish`) vs **autonomous** (bot posts without human action).

**`corky schedule run` / `corky watch` is the riskier case** — it posts autonomously when `scheduled_at <= now`. That's closer to "automating posting." Something to be aware of if LinkedIn ever scrutinizes your app.

**Other highlights relevant to you:**

1. **No charging for API access** — corky is free/open-source, so fine.
2. **100K lifetime user limit** for self-serve apps — not a concern for personal use.
3. **Token storage is allowed** — explicitly permitted. Your `tokens.json` is fine.
4. **Branding** — don't imply LinkedIn endorses corky. The corgi logo is fine.
5. **24-hour security incident reporting** — if tokens are compromised, notify LinkedIn within 24 hours.
6. **Deletion on request** — applies if you store *other users'* LinkedIn data. For posting your own content, N/A.

**Bottom line:** For personal, user-initiated posting via a CLI tool, you're fine in practice. Just be aware that `corky watch` auto-publishing is technically in the gray zone.

## User

.It's about 8 Eastern. Is this a good time to publish the post or wait until tomorrow?

## Assistant

**Wait until tomorrow morning.** Here's why:

**8 PM ET on a Tuesday is suboptimal.** LinkedIn is a professional network — people check it during work hours, not in the evening. Your post will get buried overnight as other content publishes.

**Best times for LinkedIn engagement:**
- **Tuesday–Thursday, 8–10 AM ET** — peak feed browsing during morning coffee/commute
- **Tuesday–Wednesday** tends to outperform Thursday slightly
- **Avoid Mondays** (inbox catch-up) and **Fridays** (mentally checked out)

**Recommendation: Tomorrow (Wednesday) between 8–9 AM ET.** That gives you:
1. Fresh eyes to do a final read-through of the post text
2. Peak engagement window
3. Time to manually attach the screenshot image (since `corky social publish` doesn't support images yet)

If you want to be ready to go first thing, prep tonight:
- Final review of `mail/social/20260224-agent-tooling-linkedin.md`
- Have `~/Pictures/agent-loop/2026-02-25_18-22-my-humble-setup.png` ready to upload
- Copy-paste the post body into LinkedIn's web composer, attach the image, and hit publish at ~8:30 AM ET

## User

Would a video of usage be significantly more compelling or would the screenshot have enough of an impact?
/home/brian/Pictures/agent-loop/2026-02-25_18-22-my-humble-setup.png

## Assistant

**The screenshot is enough for Post #1. Save the video for later.**

Here's the tradeoff:

**Screenshot wins for this post:**
- LinkedIn auto-plays videos but most people scroll past — completion rates on organic videos are ~20%
- Your screenshot is *instantly* legible — multiple panes, editors, terminals, the tmux layout tells the story at a glance
- Images get **2x engagement** over text-only posts. Video gets *slightly* more than images, but the production effort isn't worth the marginal bump for a first post
- A screenshot is "proof of work" — it says "this is real, this is my actual setup" without requiring 30 seconds of attention

**Video wins for a follow-up post:**
- A 30-60 second screen recording of the workflow — edit markdown → hotkey → watch the response stream back — would be *visceral*. People would understand the tool immediately
- That's Post #2 or #3 material. By then you have followers from Post #1 who are primed to watch
- Record with OBS or `wf-recorder` (Wayland) / `ffmpeg` (X11). Keep it under 60 seconds, no voiceover needed — just captions or a text overlay

**Recommendation:** Ship Post #1 tomorrow morning with the screenshot. If it gets traction (comments, reposts), record a short video for a follow-up post within a week while the audience is warm.

## User

For a 1st interview with https://www.linkedin.com/in/tobyhill/

Is this a good note to add to the Calendy appointment?

I'm working on processes & tooling for human <-> agent loops...for software development & organization intelligence.

## Assistant

**Toby Hill** is the founder of [D24 Search](https://d24search.io/) — a boutique DeepTech recruiter placing talent at venture-backed startups and VCs across North America and Europe. He's the recruiter, not the hiring company.

**Your note is good but could be sharper.** "Processes & tooling for human <-> agent loops" is insider language a recruiter may not immediately map to a job spec.

**Suggested revision:**

> Building developer tools for AI-assisted workflows — spec-driven agent orchestration, persistent AI document sessions, and automated correspondence pipelines. Rust, Python, full-stack. Open to DeepTech / AI infrastructure roles.

**Why:** Recruiter-parseable category, concrete deliverables he can use as talking points, tech stack keywords he can match against job specs, and "Open to DeepTech" matches D24's focus area.

**Short version** (if the field is small):

> AI developer tooling in Rust — agent orchestration, automated workflows, full-stack. Open to DeepTech roles.

The Calendly note is a pre-filter — give him keywords he can grep for in his job spec database.

## User

Let's use /home/brian/Pictures/agent-loop/2026-02-25_18-22-my-humble-setup.png for the linked in post.

Let's schedule the post for 8:30AM Eastern tomorrow.

## Assistant

**Two blockers for automated posting:**

1. **Image support** — `corky social publish` doesn't support image attachments yet (0.12.0 feature, not built)
2. **LinkedIn API scheduling** — no native scheduling. `corky schedule run` handles client-side scheduling but also needs image support

Well...let's add image support and make this automated.

**Manual plan for tomorrow at 8:30 AM ET:**
1. Open LinkedIn web composer
2. Copy-paste the post body from `mail/social/20260224-agent-tooling-linkedin.md` (lines 13-30, minus YAML frontmatter)
3. Attach `~/Pictures/agent-loop/2026-02-25_18-22-my-humble-setup.png`
4. Add hashtags: `#OpenSource #Rust #AITooling #AgenticDevelopment #DeveloperTools`
5. Publish

I can add `scheduled_at` and `images` fields to the draft to document intent (not functional yet — for when corky 0.12.0 lands). Want me to update the draft?

Let's make this actually automated

## Assistant

**Let's do it.** The scope for corky 0.12.0 image support:

1. `social/draft.rs` — add `images: Vec<String>` + `scheduled_at: Option<String>` to `SocialDraftMeta`
2. `social/linkedin.rs` — add `upload_image()` via REST API (`/rest/images?action=initializeUpload` + PUT), update `create_post()` for single image / carousel
3. `social/publish.rs` — resolve image paths relative to draft, upload each, pass URNs
4. Tests — IM1-IM6 edge cases
5. Update the draft with functional `images:` and `scheduled_at:` fields

Branch: `20260225/linkedin-images` in the corky submodule. This needs to happen there — want me to switch context and implement now?

## User

yes...make sure it's going to work. Testing and if possible a dry run with the Linked in api

## Assistant

**On it.** Implementation + verification plan:

1. **Build** — image support in `corky/` on branch `20260225/linkedin-images` (draft fields, REST API upload, publish flow)
2. **Unit tests** — IM1-IM6 edge cases
3. **Dry run** — `corky social publish --dry-run` uploads the image, prints the API payload, but doesn't create the post
4. **Verify** — confirm token is valid, image uploads successfully, payload looks correct
5. **Update draft** — add `images:` pointing to the screenshot

LinkedIn API has no official dry-run mode, but we can test image upload separately (upload → get URN → stop). Then the real publish happens tomorrow at 8:30 AM ET.

Switching to `corky/` submodule now to implement.

## User

Implement from here.

## Assistant

**Surprise: image support is already fully implemented in corky.** No new code needed.

The exploration found:
- `SocialDraftMeta.images: Vec<String>` already exists
- `linkedin::upload_image()` already implements the 2-step REST API flow
- `publish::upload_images()` already resolves paths and uploads before `create_post()`
- `create_post()` already handles 0 images (text), 1 image (media), 2+ images (carousel)
- Tests IM1-IM6 already pass

**What I did:**
- Updated `mail/social/20260224-agent-tooling-linkedin.md` — added `images:` field pointing to the screenshot
- Verified: draft parses correctly, screenshot exists (695KB, well under LinkedIn limits)

**One remaining blocker: No LinkedIn auth token.** The profiles.toml has your `btakita` profile but no `tokens.json` exists yet. You need to complete the OAuth flow:

```bash
corky social auth linkedin --profile btakita
```

This opens your browser → LinkedIn approval page → redirects to `127.0.0.1:8484/callback` → token saved.

**After auth, the publish flow is:**
```bash
# Change status to ready
sed -i 's/status: draft/status: ready/' mail/social/20260224-agent-tooling-linkedin.md

# Publish (uploads image + creates post)
corky social publish mail/social/20260224-agent-tooling-linkedin.md
```

No `--dry-run` flag exists for `social publish` currently. If you want to test the image upload without posting, we can add that. Otherwise: auth → change status → publish tomorrow at 8:30 AM ET.

## User

Authenticated...Let's implement --dry-run

## Assistant

Done. `--dry-run` is implemented and verified.

**Changes (5 files):**

1. **`cli.rs`** — Added `--dry-run` flag to `Publish` subcommand
2. **`main.rs`** — Pass `dry_run` through to `run_publish`
3. **`social/mod.rs`** — Updated `run_publish` signature
4. **`social/publish.rs`** — Dry-run validates auth, uploads images (to verify they work), prints the full payload, then stops before `create_post()`. Also allows `status: draft` in dry-run mode so you don't need to flip to `ready` just to test.
5. **`social/linkedin.rs`** — Updated `LinkedIn-Version` from expired `202401` to `202601` (the old version was returning HTTP 426)
6. **`schedule.rs`** — Pass `false` for the new parameter (scheduled publishes are always real)

**Dry-run result:**
```
[dry-run] Validation passed. Would publish to linkedin.
[dry-run] Author: btakita (urn:li:person:_C2t_TDOVY)
[dry-run] Visibility: public
[dry-run] Images uploaded: 1
[dry-run]   1: urn:li:image:D4E10AQHt0WDgT2usRw
[dry-run] Body (1880 chars):
---
Building my own agent tooling — and eating my own dog food
...
---
[dry-run] No post created. Set status to 'ready' and run without --dry-run to publish.
```

Auth is valid. Image uploads successfully. Body is 1880 chars (well under 3000 limit). Everything checks out.

**To publish tomorrow at 8:30 AM ET:**
```bash
# 1. Change status to ready
sed -i 's/status: draft/status: ready/' mail/social/20260224-agent-tooling-linkedin.md

# 2. Publish
corky social publish mail/social/20260224-agent-tooling-linkedin.md
```

## User

Scheduling the linked in post should have changed status to ready.

The scheduler should run corky social publish when the time is hit. It should also prevent posting the same post multiple times.

## Assistant

Good call — you're right, requiring a separate `status: ready` flip when `scheduled_at` is already set is redundant. Fixed.

**Changes:**

1. **`schedule.rs` — scanner**: Now accepts `draft` or `ready` status for scheduled items. Only skips `published` (preventing double-publish). Previously required `status: ready`.

2. **`publish.rs` — status check**: `draft` status is now allowed when `scheduled_at` is set. The logic:
  - `published` → always reject (prevents double-publish)
  - `draft` + `scheduled_at` set → allowed (scheduling implies readiness)
  - `draft` + no `scheduled_at` → reject (manual publish still requires `ready`)
  - `ready` → always allowed

3. **Tests**: S8 updated from "draft status skipped" to "draft with schedule picked up". Added S8b explicitly testing that `published` status is skipped.

4. **Draft updated**: Added `scheduled_at: "2026-02-26T13:30:00Z"` (8:30 AM ET) to the LinkedIn post. Status stays `draft` — the scheduler will pick it up when the time hits.

**Verification:**
```
$ corky schedule list
  2026-02-26 13:30 UTC social — linkedin (mail/social/20260224-agent-tooling-linkedin.md)

$ corky schedule run --dry-run
No scheduled items due.   # correct — it's tomorrow
```

All 132 tests pass. The publish flow already sets `status: published` after posting, so double-publish is naturally prevented — the scanner will skip it on the next run.

**To publish**: Run `corky schedule run` (or `corky watch`) tomorrow at or after 8:30 AM ET. Or let `corky watch` handle it automatically if you have it running.

## User

Is corky watch running. Will it handle the schedule?

## Assistant

**No, `corky watch` is not running.** No corky processes found.

Three options to publish at 8:30 AM ET:

1. **Start `corky watch` now** — runs in a loop (IMAP sync + schedule check), picks up the post automatically:
   ```bash
   corky watch &
   ```

2. **One-shot cron/at** — fire at the exact time:
   ```bash
   echo "corky schedule run" | at 08:30 AM 02/26/2026
   ```

3. **Run manually tomorrow** — `corky schedule run` at or after 8:30 AM ET.

Option 1 is the most hands-off. `corky watch` checks the schedule on each loop iteration, so as long as it's running by tomorrow morning, it'll fire the post.

## User

Does corky watch need to run from a particular directory?

## Assistant

**Yes.** `corky watch` uses relative path resolution — `resolve::data_dir()` looks for `mail/` in the current working directory first. For your setup, run it from the agent-loop root where the `mail/` symlink lives:

```bash
cd /home/brian/work/btakita/agent-loop && corky watch &
```

Or set `CORKY_DATA` so it works from any directory:
```bash
export CORKY_DATA=/home/brian/work/btakita/agent-loop/mail
corky watch &
```

The resolution order is: `./mail/` → `$CORKY_DATA` → app config mailbox → `~/Documents/mail`.

## User

Post sent out successfully. So far no interaction.

## Assistant

Congrats on the post going live. Engagement takes time — LinkedIn's algorithm seeds morning posts over 2-4 hours.
