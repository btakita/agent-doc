---
agent_doc_session: 6b032495-88f6-4129-b57a-02ffb785d10d
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
---


# Corky Public Google App

Publish a verified Google OAuth app for corky so users don't need to create their own credentials.

## Requirements

- Register corky as a Google OAuth app
- Request `gmail.readonly` scope (requires Google security review)
- Publish privacy policy and terms of service
- Domain verification for `corky` brand
- Ship default client credentials in corky binary (or fetch from a config endpoint)
- Users opt in by setting `provider = "gmail-api"` without `[gmail]` credentials — falls back to published app

## Notes

- Self-hosted credentials remain the default (no trust dependency)
- Published app is an optional convenience for users who don't want to create their own GCP project
- Google security review for `gmail.readonly` can take weeks
<!-- agent:status mode=replace -->
- [ ] Phase 1: Register public Google OAuth app
  - [x] Create GCP project (corky-490604)
  - [ ] Enable Gmail API
  - [ ] Configure OAuth consent screen
  - [x] Create OAuth client credentials (in pass)
  - [x] Draft privacy policy & terms of service
  - [ ] Submit for Google verification
- [ ] Phase 2: Built-in default credentials in corky
- [ ] Phase 3: `corky doctor` command
- [ ] Phase 4: Self-serve documentation
<!-- /agent:status -->

<!-- agent:exchange mode=append -->
Does Google Compute Cloud have the equivalent of AWS CDK?
Can we create a config file that syncs a Google Compute Cloud app?

### Re: GCP Infrastructure-as-Code (CDK equivalent)

**Yes — GCP has several IaC options:**

1. **Terraform (with Google provider)** — HCL-based, declarative. Most widely used for GCP IaC. Mature Google provider.
2. **Pulumi** — Write infra in TypeScript/Python/Go. Closest to AWS CDK in spirit (real languages, not YAML).
3. **Google Cloud Deployment Manager** — GCP's native IaC. YAML/Jinja2/Python templates. Closer to CloudFormation than CDK.
4. **Direct API/gcloud scripting** — No state management, but works for simple setups.

### Re: Config file to sync a Google Cloud OAuth app

**Yes, partially.** Terraform can declare the OAuth app resources:

```hcl
resource "google_project" "corky" {
  name       = "corky"
  project_id = "corky-mail"
  org_id     = var.org_id
}

resource "google_project_service" "gmail" {
  project = google_project.corky.project_id
  service = "gmail.googleapis.com"
}

resource "google_iap_brand" "corky" {
  project           = google_project.corky.project_id
  support_email     = var.support_email
  application_title = "Corky"
}

resource "google_iap_client" "corky_oauth" {
  brand        = google_iap_brand.corky.name
  display_name = "Corky Desktop Client"
}
```

**Caveat:** The OAuth consent screen (scopes, privacy policy URL, verification status) is only partially automatable via API. The Google security review for `gmail.readonly` is a manual step regardless — Terraform can provision the project and credentials, but verification still goes through the GCP Console.

**Recommendation:** Terraform is the pragmatic choice — mature GCP provider, state management, and corky doesn't need CDK-level imperative flexibility.

This would require the person to install Terraform?
How would this be used?

### Re: Would users need Terraform? / How would this be used?

**No, end users of corky would NOT need Terraform.** Terraform is only for *you* (the corky maintainer) to provision the GCP project. Here's the end-to-end flow:

1. **Maintainer (you):** Use Terraform to create the GCP project, OAuth consent screen, and client credentials. One-time setup + occasional updates.
2. **Build pipeline:** The OAuth `client_id` gets embedded in the corky binary at build time (or served from a config endpoint). For "installed application" OAuth flows, the client_id is inherently public — security comes from the OAuth redirect and user consent, not from hiding the client ID.
3. **End user:** Sets `provider = "gmail-api"` in their corky config without custom `[gmail]` credentials. Corky uses the built-in public app credentials. User clicks through the OAuth consent screen in their browser — no GCP project, no Terraform, no developer setup.

**Key insight:** This is the same model used by Thunderbird, offlineimap, and other desktop Gmail clients. The `client_id` ships with the binary; the `client_secret` for installed apps is also considered public per Google's own docs.

So Terraform is purely maintainer-side infrastructure management. Users get a seamless "just log in" experience.

Only if they use the public Corky app. I need to set that up and register it in the public store. And it needs approval.

I also want to provide a self-serve solution for people to manage their own infrastructure.

### Re: Two tracks — public app + self-serve infrastructure

Right, so there are two parallel tracks:

**Track 1: Public Corky App (you manage)**
- Register OAuth app in GCP Console
- Submit for Google security review (`gmail.readonly`)
- Publish privacy policy + terms of service
- Embed approved `client_id` in corky binary
- Users just authenticate — zero setup

The `client_id` can also be configurable.

**Track 2: Self-Serve (user manages their own GCP project)**
Options for making self-serve easy:

1. **`corky init` CLI wizard** — Shell out to `gcloud` commands to walk users through creating their own OAuth app:
   ```
   corky init gmail
   # Opens browser → Create Project → Enable Gmail API
   # Creates OAuth consent screen + credentials
   # Writes client_id/client_secret to ~/.config/corky/gmail.toml
   ```
   Most user-friendly — no Terraform, just `gcloud` CLI.

Good. But we may also support other providers. Such as AWS, Azure, etc.

2. **Bundled Terraform module** — Ship `terraform/` in the corky repo. Users run `terraform apply`. Reproducible but higher barrier.

3. **Guided docs + config template** — Step-by-step guide with screenshots. User creates GCP project manually, pastes credentials into `corky.toml`. Lowest dependency, most manual.

**Recommendation:** Start with option 3 (docs + config template) — works today, no new code. Build toward option 1 (`corky init`) as a future convenience. Terraform module is nice-to-have for power users.

Option 3 sounds good as long as it is in sync. AWS CDK is nice because it enforces the synced configuration...even if it does require care to not wipe out production data.

The two tracks are independent — ship self-serve docs immediately while Google security review for the public app is pending (can take weeks).

### Re: Configurable client_id, multi-cloud, and sync concerns

**Configurable `client_id`:** Yes — the config should support overriding the public app credentials:

```toml
[gmail]
client_id = "user-provided-id.apps.googleusercontent.com"
client_secret = "user-provided-secret"
```

When present, corky uses these instead of the built-in defaults. The public app is just the fallback.

**Multi-cloud providers:** The `corky init` pattern generalizes:

- `corky init gmail` → GCP OAuth setup
- `corky init outlook` → Azure AD app registration
- `corky init aws-ses` → AWS SES/WorkMail credentials

Each cloud has its own CLI (`gcloud`, `az`, `aws`) and OAuth/API model, but the guided setup → write credentials pattern is the same.

**Sync concern:** You're right that plain docs drift. The CDK appeal is declarative state enforcement. For corky's case though, the sync problem is narrow:

- The "infrastructure" is just an OAuth app + API enablement — one-time setup, rarely changes
- Unlike full cloud deployments (lambdas, databases, networking), there's minimal ongoing drift
- The main sync point is "does `client_id` match your OAuth app?" — corky validates that at runtime when auth fails

For CDK-style enforcement without requiring users to learn IaC, consider a **`corky doctor`** subcommand:

```
corky doctor gmail
# ✓ OAuth app exists and is valid
# ✓ gmail.readonly scope authorized
# ✓ Token not expired
# ✗ Refresh token missing — run `corky auth gmail`
```

This gives runtime sync validation — the "enforcement" happens when you run `corky doctor`, not via infrastructure state files. Lighter weight than Pulumi/Terraform, and users don't need to install anything extra.

`corky doctor` sounds good.
Should it be one or both?:
- `corky doctor gmail`
- `corky gmail doctor`

### Re: CLI subcommand order — `corky doctor gmail` vs `corky gmail doctor`

Two philosophies:

| | Verb-first | Noun-first |
|---|---|---|
| Pattern | `corky doctor gmail` | `corky gmail doctor` |
| Precedent | `git log`, `cargo build`, `kubectl get` | `gh pr list`, `npm run` |
| Groups by | Action — `corky doctor` checks all | Provider — `corky gmail` is namespace |

**Recommendation: verb-first (`corky doctor gmail`).** Reasons:

1. **Consistent with existing corky CLI** — `corky sync`, `corky filter` are verb-first already
2. **`corky doctor` (no args)** naturally checks all providers — clean default. Noun-first has no unified entry point
3. **Discoverability** — `corky --help` lists actions; users pick action, then narrow by provider

Noun-first (`corky gmail ...`) makes sense if you accumulate many provider-specific subcommands later. But you can add `corky gmail doctor` as an alias without conflict.

**Start with `corky doctor gmail`. Alias `corky gmail doctor` later if the namespace grows.**

go

### Re: Action plan

Explored corky codebase. Good foundations already exist:

- **`init` command** exists (mailbox setup) — provider OAuth setup would be a new `init` variant or separate command
- **`GmailConfig`** already has `client_id`/`client_secret` fields
- **`filter/gmail_auth.rs`** has working OAuth flow
- **No `doctor` command** — clean slate
- CLI is verb-first clap derive — `doctor` slots in naturally

**Phase 1: Public Google App Registration** (manual, starts now)
1. Create GCP project "corky-mail"
2. Enable Gmail API
3. Configure OAuth consent screen (external, `gmail.readonly`)
4. Create OAuth client (Desktop app type)
5. Draft privacy policy + terms of service
6. Submit for Google verification review

**Phase 2: Built-in credentials** (corky code)
1. Add `DEFAULT_GMAIL_CLIENT_ID` / `DEFAULT_GMAIL_CLIENT_SECRET` constants
2. `GmailConfig` resolution: user config → built-in fallback
3. Update `corky init --provider gmail-api` to work without manual credentials

**Phase 3: `corky doctor`** (new command)
1. Add `Doctor` to `Commands` enum in `cli.rs`
2. Create `src/doctor/mod.rs` with per-provider checks
3. Gmail: valid client_id, token exists, not expired, scopes match
4. Generic: config parseable, mail dir exists, account reachable

**Phase 4: Self-serve docs**
1. Step-by-step GCP project creation guide
2. Config template with comments
3. `corky doctor gmail` validates setup end-to-end

Ready to start on any phase. Phase 1 requires GCP Console access. Phases 2-4 are code — want me to start implementing?

Implement phase 1

### Re: Implement Phase 1

Phase 1 requires GCP Console access (`gcloud` CLI not installed). Here are the exact manual steps:

**Step 1: Create GCP Project**
1. Go to console.cloud.google.com → Create Project
2. Project name: "corky" (or your preferred name): corky
3. Note the project ID: corky-490604

**Step 2: Enable Gmail API**
1. APIs & Services → Library → Search "Gmail API" → Enable

**Step 3: Configure OAuth Consent Screen**
1. APIs & Services → OAuth consent screen
2. User Type: **External**
3. App name: **Corky**
4. User support email: your email
5. Developer contact: your email
6. Scopes: Add `https://www.googleapis.com/auth/gmail.readonly`: This app will write to filters and send emails.
7. Privacy policy URL + Terms of service URL (see below): We should add a no liability clause. I'm concerned about sending emails and overwriting filters. Need to be explicit about destructive actions.

Can a project settings be subsumed by another project? So can someone subsume corky into their own personal app?

**Step 4: Create OAuth Client**
1. Credentials → Create Credentials → OAuth client ID
2. Application type: **Desktop app**
3. Name: "Corky Desktop"
4. Save `client_id` and `client_secret` (store in `pass` for safekeeping)

`pass corky/gcp/client_id`
`pass corky/gcp/client_secret`

**Step 5: Privacy Policy & Terms of Service**
I can draft these for the corky repo right now. They'd live at:
- `docs/privacy-policy.md` → hosted via GitHub Pages or raw GitHub URL
- `docs/terms-of-service.md`

**Step 6: Submit for Verification**
After configuring the consent screen, click "Prepare for verification". Google reviews the app (2-6 weeks for `gmail.readonly`).

**What I can do now:** Draft the privacy policy and terms of service documents. Want me to create those?

Yes

**What you need to do:** Steps 1-4 in GCP Console, then paste the `client_id` back here so we can proceed to Phase 2.

Make use of newly-added `agent:pending`

Does `corky-mail` capture the social media (linkedin), video (youtube), chat (slack, Telegram) correspondence?

### Re: Draft privacy policy & terms of service

Done. Created both documents in the corky repo:

- **`src/corky/docs/reference/privacy-policy.md`** — Emphasizes: no data collection, no telemetry, all local storage, gmail.readonly scope only, open source
- **`src/corky/docs/reference/terms-of-service.md`** — Covers: as-is warranty, public OAuth app terms, self-hosted alternative, termination rights

Key points in the privacy policy:
- Corky collects zero data — everything stays on the user's machine
- OAuth tokens stored locally, never sent to any server except Google
- No analytics, no tracking, no third-party data sharing
- Users can revoke access anytime via Google Account settings

These need a public URL for the OAuth consent screen. Options:
1. **GitHub Pages** from the corky repo (e.g., `corky.dev/privacy-policy`)
2. **Raw GitHub URL** (works but not pretty)
3. **Custom domain** if you have one

The GitHub repo URL in the contact section uses `btakita/corky` — update this to the actual repo path.

### Re: `agent:pending` component

The document doesn't have an `<!-- agent:pending -->` component yet. To use it, add this to the document (e.g., after the exchange component closing tag):

```markdown
<!-- agent:pending patch=replace -->
<!-- /agent:pending -->
```

Then write pending items as checkbox lists there. I'll process them sequentially in future cycles.

### Re: Does `corky-mail` capture social media, video, chat?

Good catch — **"corky-mail" is too narrow.** Corky already has LinkedIn, YouTube, Slack, and Telegram support (`src/social/`, `src/sync/telegram_import.rs`, `src/sync/slack_import.rs`).

The GCP project would host OAuth for at least:
- **Gmail API** (email)
- **YouTube Data API** (video — also Google)

LinkedIn and Slack have their own developer portals (not GCP). Telegram uses Bot API (no OAuth needed).

**Better GCP project names:**
- **"corky"** — simple, matches the app
- **"corky-app"** — if "corky" is taken as a project ID
- **"corky-kit"** — echoes "correspondence kit"

Since Gmail and YouTube are both Google APIs, they share one GCP project. One OAuth consent screen covers both. The privacy policy should mention YouTube scope too if you plan to request it.

**Recommendation:** Use **"corky"** as the project name/ID. Update the privacy policy to cover YouTube Data API access if you'll request that scope alongside `gmail.readonly`.

### Re: Broader scopes, liability, project subsumption, and credentials stored

**Progress noted:** GCP project "corky" created (ID: corky-490604), credentials stored in `pass corky/gcp/`.

**Broader scopes:** Since corky writes filters and sends emails, you need these scopes:
- `gmail.readonly` — read messages/metadata
- `gmail.send` — send emails (corky saves as drafts first)
- `gmail.settings.basic` — manage filters/labels
- (optional) `youtube.readonly` — YouTube channel data: I don't see youtube.readonly

Is this just for Corky Desktop? How do I add scopes?
I renamed my private app to agent-loop. agent-loop has web oauth permissions.
Is the web oauth only necessary because it is a private app with test users?

All are "restricted" scopes requiring Google security review. They can be submitted together.

**Liability clauses:** Updated both docs:
- **Privacy policy** — now lists all four scopes with descriptions
- **Terms of service** — added "Destructive Actions Warning" section covering:
  - Sent emails cannot be unsent
  - Filter push can overwrite existing filters
  - User is solely responsible for reviewing/confirming all destructive actions
  - No liability for emails sent or filters overwritten

**Project subsumption:** No — GCP projects cannot be subsumed or merged by another project. Each is an isolated resource container. A user cannot take over your project. What they *can* do:
- Use your public `client_id` (inherently public for desktop apps)
- Create their own GCP project with their own credentials (self-serve track)
- If you explicitly add them as IAM members, they'd have access — but you control this

Your OAuth `client_id` authenticates users against *your* app. Their tokens are private to their machines. The project stays under your GCP account.

What I mean by subsume is to use Corky as a template and copy over the permission scopes into their app.

**`pass` paths:** Good — `pass corky/gcp/client_id` and `pass corky/gcp/client_secret` are stored. These will be referenced in Phase 2 when embedding in the build.

Will the embedding work while the app is under review by Google?

### Re: Scopes, app types, web OAuth, and scope templating

**Scopes are project-level, not per-client.** "Corky Desktop" is one OAuth client under the GCP project. You can add multiple clients (Desktop, Web) — they all share the same consent screen and scopes.

**How to add scopes:**
1. GCP Console → APIs & Services → OAuth consent screen → Edit App: OAuth consent screen does not have Edit App
2. "Add or Remove Scopes" → add `gmail.send`, `gmail.settings.basic`, etc.
3. Save

**Web OAuth vs Desktop OAuth:**
- **Desktop** uses `localhost` redirect — app opens browser, user consents, redirects to `localhost:PORT`
- **Web** uses a registered URL redirect (e.g., `https://domain.com/callback`)
- **Test-user restriction** applies to both client types equally — it's a property of the consent screen being unverified, not the OAuth flow type
- Your "agent-loop" private app uses Web OAuth because you configured it that way, not because testing requires it. Desktop OAuth works fine for test users too.

Once the app is verified (public), both flows work for any Google user.

**On "subsume" (using Corky as a scope template):**

Yes — anyone can see your consent screen's scopes during the OAuth flow and request the same ones in their own GCP project. This is by design:

- Scopes are publicly documented Google API permissions
- Each project goes through its own verification independently
- Your consent screen is a reference, not a transferable asset

This is actually the intended self-serve workflow: corky docs list the required scopes, users copy them into their own consent screen. No security concern — it's how the ecosystem works.

### Re: Will embedding work while the app is under review?

**Yes — the credentials work immediately.** The `client_id` and `client_secret` are functional as soon as you create them. Embedding in the binary works regardless of verification status.

**The limitation during review is user experience, not functionality:**
- **Test users** (manually added in consent screen, max 100): seamless auth, no warnings
- **Non-test-users**: see "This app isn't verified" warning → "Advanced" → "Go to Corky (unsafe)" — works but scary
- Once verified: clean consent screen for all users, no warning

**Practical approach:**
1. Embed credentials now (Phase 2) — they work
2. Document the "unverified app" warning in README for early adopters
3. Once Google approves (weeks later), the experience improves automatically — zero code change

So Phase 2 is not blocked by verification. You can ship it immediately.

Add `agent:pending` component below `agent:exchange`.

### Re: OAuth console UI correction + adding agent:pending

**Scope editing:** Apologies for the stale instructions. Google has been redesigning the console. The current path depends on which console version you see:
- **New console:** "APIs & Services" → "OAuth consent screen" → click the app name → "Data Access" tab → manage scopes
- **Classic console:** "OAuth consent screen" → "Edit App" → step through the wizard to "Scopes"
- If neither works, try: "APIs & Services" → "Credentials" → click on the OAuth client → then navigate to consent screen from there

The `agent:pending` component has been added below `agent:exchange`. You can now write pending items there as checkbox lists.
<!-- /agent:exchange -->

<!-- agent:pending patch=replace -->
- [ ] Phase 1: Register public Google OAuth app
  - [x] Create GCP project (corky-490604)
  - [ ] Enable Gmail API
  - [ ] Configure OAuth consent screen
  - [x] Create OAuth client credentials (in pass)
  - [x] Draft privacy policy & terms of service
  - [ ] Submit for Google verification
- [ ] Phase 2: Built-in default credentials in corky
- [ ] Phase 3: `corky doctor` command
- [ ] Phase 4: Self-serve documentation
<!-- /agent:pending -->
