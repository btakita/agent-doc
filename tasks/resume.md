# Resume Session

## Status

**Resume**: `resume/BrianTakita.md` — committed and pushed to [btakita/resume](https://github.com/btakita/resume)
**PDF**: auto-generated via GitHub Action, deployed to [btakita.github.io/resume](https://btakita.github.io/resume/)
**LinkedIn**: [linkedin.com/in/briantakita](https://www.linkedin.com/in/briantakita/)

## Completed

- Markdown resume created from LinkedIn PDF export + new content
- Resume repo: public at `btakita/resume`, submodule of agent-loop
- PDF pipeline: Makefile (`make pdf`) → pandoc + weasyprint with `resume.css`
- GitHub Action: triggers on .md/.css/Makefile changes, builds PDF, commits, deploys to Pages
- GitHub Pages: landing page at btakita.github.io/resume with PDF download
- README: workflow docs, build instructions, PDF link

### Content Changes

- **Presence AI**: dates updated to April 2024 – February 2026
- **Summary**: "Most recently built the full real-time AI avatar pipeline at Presence AI. Now focused on spec-driven agent tooling and collaborative intelligence workflows."
- **Tagline**: "Building emergent software systems through agent loops — spec-driven development, automated verification, and collaborative intelligence tooling where agents and humans work in parallel with full context. 20+ years full stack experience."
- **Top Skills**: Spec-Driven Development, Agents, Rust, Python, TypeScript, Zig
- **Open Source section**: 2002 – Present
  - **corky** — Multi-Source Correspondence Toolkit for Agent-Augmented Collaboration (Rust)
  - **agent-doc** — Document-as-Interface for Interactive Sessions with AI Agents (Rust)
  - Other contributions (italic, visually lighter): RSpec, SvelteJS, RMemo/RelementJS, rappstack, Access Query Analyzer, lazily-py, lazily-zig
- **Proofread**: 13 fixes (grammar, tense, parallelism, style)
- **SLOC metric removed** from corky heading
- **`| Docs` link removed** from corky GitHub line

### LinkedIn Drafts

**About section** (copy-paste ready):
Over 20 years of software development experience developing full-stack applications. Most recently built the full real-time AI avatar pipeline at Presence AI — facial landmark detection, LatentSync lip-sync inference, and frame sequencing on H100 GPUs. Now building spec-driven agent tooling and collaborative intelligence workflows in Rust.

Utilizes philosophy, logic, language, & qualitative/quantitative approaches to create software models to explore & accurately represent the domain.

Currently dog-fooding two open-source tools: corky (git-native correspondence toolkit) and agent-doc (persistent AI document sessions) — both built through the same spec-driven agent methodology they enable.

**LinkedIn post** (copy-paste ready):
> **Building my own agent tooling — and eating my own dog food**
>
> I've been building two open-source Rust tools that address gaps I kept hitting in my own agentic development workflow:
>
> **corky** — a git-native correspondence toolkit that syncs topics, contacts, and threads from email (IMAP), Telegram, and Slack into scoped markdown mailboxes. Each collaborator gets only the threads and AI context relevant to them, with bidirectional sync via 3-way merge. I use it daily for my personal and professional correspondence.
>
> **agent-doc** — a document-as-interface for persistent AI conversations. Edit a markdown file offline, press a hotkey, and the tool diffs your changes, sends them to an AI agent, and writes the response back inline. Git tracks every exchange. I use it as my primary planning and iteration tool — including for writing this post.
>
> Corky started as a Python prototype. I ported it to Rust in under a day using spec-driven development: write a detailed spec first, let the agent implement against it, audit the output against the spec. The specs become living documents that evolve with the code.
>
> With agent-doc, I run multiple agent sessions in parallel across different task documents — each one a coherent, persistent conversation. When a session gets long, I compact it: reorganize, delete noise, keep the signal. The agent sees the cleaned-up document as ground truth on the next run.
>
> 100% dog-fooding. 100% open source. 100% spec-driven development.
>
> corky: https://github.com/btakita/corky
> agent-doc: https://github.com/btakita/agent-doc
>
> The biggest lesson: when you use your own tools every day, the feedback loop is instant. Bugs become personal. Missing features become obvious. The quality bar rises because you're the first user who's annoyed by every rough edge.
>
> #OpenSource #Rust #AITooling #AgenticDevelopment #DeveloperTools

## Pending

- LinkedIn API: Posts API supports creating posts (`POST /v2/posts`, scope `w_member_social`). OAuth flow needed for token.
Let's add a way to post to LinkedIn. Corky would be a good place to add this capability. Other social media will be supported in the future. See if there's any logic that can be reused.
- ~PAT management: fine-grained PAT "lucas" needs `btakita/resume` added via web UI (Settings > Tokens > Edit)~ Done

## User



## Assistant

### LinkedIn Posting via Corky — Design

**Yes, corky is the right home.** It already handles correspondence across email/Telegram/Slack — social media posting is a natural extension of "outbound correspondence."

**API Summary:**
- **Endpoint:** `POST https://api.linkedin.com/rest/posts` — text, images, articles all supported
- **OAuth scopes:** `w_member_social` + `openid` + `profile` — all self-service, no approval needed
- **Token lifetime:** 60 days (refresh tokens are partner-restricted, so re-auth every ~2 months via browser)
- **Rate limits:** ~100 req/day per user token; 3-5 posts/day practical ceiling

**OAuth flow for CLI:**
1. Spin up local HTTP server on `127.0.0.1:8484/callback`
2. Open browser to LinkedIn authorization URL
3. Capture the `?code=` redirect
4. Exchange for access token, store in `~/.config/corky/tokens.json`
5. Auto-detect expiry, prompt re-auth when needed

**Multi-platform architecture** (for future Twitter/X, Mastodon, Bluesky):
```
corky/src/social/
  mod.rs          — SocialPlatform trait
  linkedin.rs     — reqwest-based LinkedIn Posts API
  bluesky.rs      — AT Protocol (future)
  mastodon.rs     — Mastodon API (future)
  auth/
    oauth2.rs     — Shared OAuth2 flow (local callback server)
    token_store.rs — Encrypted token storage
```

**No unified Rust crate exists** for multi-platform social posting. `gen-linkedin` (v0.1.0) exists but is too immature — raw `reqwest` with a thin wrapper is better.

**Proposed commands:**
```bash
corky social auth linkedin       # OAuth flow, store token
corky social post linkedin       # Post from stdin or --file
corky social post linkedin --draft  # Preview without posting
```

**Implementation order:**
1. `SocialPlatform` trait + token store
2. LinkedIn OAuth flow (local callback server)
3. `corky social post linkedin` — text posts
4. Future: Bluesky, Mastodon, Twitter/X

**LinkedIn app setup required first:**
1. Create app at [linkedin.com/developers](https://www.linkedin.com/developers/)
2. Enable "Share on LinkedIn" + "Sign In with LinkedIn using OpenID Connect" products
3. Add `http://127.0.0.1:8484/callback` as redirect URI
4. Note client_id and client_secret

## User

Would LinkedIn in any way penalize a post created by an app vs having the actual user create the post?

## Assistant

### No confirmed penalty for API-created posts

**Official stance:** LinkedIn's documentation treats API posts as fully equivalent to native posts. The Posts API uses `"feedDistribution": "MAIN_FEED"` — same distribution pipeline as manually created posts. No official docs mention reduced reach for API-sourced content.

**No "posted via" label:** Unlike Twitter/X (which used to show "via TweetDeck"), LinkedIn does **not** display a visible source attribution on API-created posts. Viewers can't tell the difference.

**Community evidence is mixed but mostly favorable:**
- **Buffer's experiment:** API/third-party posts averaged *higher* reach (7,024) than native posts (6,071). Conclusion: "no significant difference."
- **Richard van der Blom (2024 algorithm report):** Claimed third-party scheduling tools were penalized, but based on a paid report with limited public data.
- **2025 consensus:** Multiple sources (Botdog, Agorapulse, Social Status) indicate any such penalty was **removed** by 2025.

**Bottom line:** Safe to post via the API. No visible "posted via app" label, no confirmed algorithmic penalty, and the dominant view is that any historical penalty has been reversed.

## User

What is the detailed design plan for LinkedIn posting via a draft?
Propose the draft frontmatter for a linkedin post?
Will the permissions be based on who has the token? So a collaborator can directly post if the collaborator has the token?

Edge Case: What if the collaborator has their own LinkedIn account token or multiple tokens? Would the author on the frontmatter need to match the linkedin profile name? 

Let's discuss...

## Assistant

### Draft Workflow — Detailed Design

**Lifecycle:** `draft` → `ready` → `published`

A social post is a markdown file in the corky mailbox, with frontmatter controlling its state:

```yaml
---
platform: linkedin
author: brian           # corky profile name (maps to token)
visibility: public     # public | connections
status: draft          # draft | ready | published
tags: ["OpenSource", "Rust", "AITooling"]
scheduled_at: null     # optional: ISO 8601 for future publish
published_at: null     # set after publish
post_id: null          # LinkedIn URN, set after publish
post_url: null         # permalink, set after publish
---
```

**File location:** `social/drafts/linkedin/2026-02-24-agent-tooling.md`

The body is the post content in plain markdown. On publish, corky converts markdown to LinkedIn's plain-text format (strips formatting, preserves links and line breaks).

**Commands:**

```bash
# Create a new draft from template
corky social draft linkedin --title "agent-tooling"
# → creates social/drafts/linkedin/2026-02-24-agent-tooling.md

# Preview (renders as LinkedIn would display it)
corky social preview social/drafts/linkedin/2026-02-24-agent-tooling.md

# Publish (changes status draft→published, calls API)
corky social publish social/drafts/linkedin/2026-02-24-agent-tooling.md

# List all drafts
corky social list --status draft
corky social list --status published
```

### Permissions — Token-Based, Multi-Account

**Yes, permissions are entirely token-based.** Whoever has a valid OAuth token for a LinkedIn account can post to it.

**Token store design** (`~/.config/corky/tokens.json`):

```json
{
  "profiles": {
    "brian": {
      "platform": "linkedin",
      "person_urn": "urn:li:person:abc123",
      "display_name": "Brian Takita",
      "access_token": "AQUvlL_...",
      "expires_at": "2026-04-25T00:00:00Z"
    },
    "company-page": {
      "platform": "linkedin",
      "person_urn": "urn:li:organization:xyz789",
      "display_name": "My Company",
      "access_token": "AQXyz...",
      "expires_at": "2026-04-25T00:00:00Z"
    }
  },
  "default_profile": "brian"
}
```

**Resolution chain for `author` field:**
1. `author: brian` → look up `profiles.brian` in token store → use that token
2. `author:` omitted → use `default_profile`
3. `author: unknown` → error: "no token found for profile 'unknown'. Run `corky social auth linkedin --profile unknown`"

### Edge Case: Multiple LinkedIn Accounts

**The `author` field references a corky profile name, not a LinkedIn display name.** This decouples the draft from the specific LinkedIn account:

- `author: brian` → Brian's personal LinkedIn
- `author: brian-company` → Brian posting as a company page admin
- `author: alice` → Alice's LinkedIn (if Alice has authed on this machine)

**Collaborator scenario:**
If Alice and Brian share a corky mailbox (via git), they can each auth their own tokens locally:

```bash
# Alice's machine
corky social auth linkedin --profile alice
# Brian's machine
corky social auth linkedin --profile brian
```

Draft files in git have `author: alice` or `author: brian`. Each person can only publish drafts that match a token they have locally. The token store is **never committed to git** — it's local-only in `~/.config/corky/`.

**No name matching needed.** The `author` field is a corky profile key, not a LinkedIn profile name. The mapping is: `author` → profile key → token → LinkedIn API call with the token's `person_urn`. LinkedIn doesn't care what you call the profile locally.

**Conflict prevention:** If Alice writes a draft with `author: brian`, she can't publish it unless she has Brian's token. She'd get: "No token for profile 'brian'. Ask Brian to publish, or change author to your profile."

### Summary Table

| Scenario | What happens |
|---|---|
| Single user, one account | `author` defaults to `default_profile` |
| Single user, personal + company page | Two profiles in token store, specify in frontmatter |
| Two collaborators sharing mailbox | Each has own token store locally, `author` determines who publishes |
| Wrong author for available token | Error with clear message |
| Token expired | Prompt to re-auth: `corky social auth linkedin --profile <name>` |
| Draft without `author` | Uses `default_profile` from token store |

## User

Do we need to ensure that each network of collaborators have unique account names? What if another brian were to join the network of collaborators? How can we detect name conflicts? Would we need a distributed algorithm or would this need to be centralized? Or would a uid appended to the name be better if there is a conflict?

What happens if a duplicate username were created...but is only conflicting when joining as a collaborator?
