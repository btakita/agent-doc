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
  - [x] Enable Gmail API
  - [x] Configure OAuth consent screen (scopes added)
  - [x] Create OAuth client credentials (in pass)
  - [x] Draft privacy policy & terms of service
  - [x] Push docs to corky repo (deploying via GitHub Pages)
  - [ ] Enter URLs in consent screen
  - [ ] Submit for Google verification
- [ ] Phase 2: Built-in default credentials in corky
- [ ] Phase 3: `corky doctor` command
- [ ] Phase 4: Self-serve documentation
<!-- /agent:status -->

<!-- agent:exchange mode=append -->
*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/aab8e49aea61417e3521b779b6c7cb27283e0b8f17d03f0d4285ff625b1a4920-20260318-060920.md`*

show instructions for adding privacy

### Re: Instructions for adding privacy policy URLs

**Step-by-step for the current GCP Console:**

1. Go to `console.cloud.google.com`
2. Select project **"corky"** (corky-490604) in the top dropdown
3. Left sidebar → **"APIs & Services"** → **"OAuth consent screen"**
4. Look for tabs across the top. Click **"Branding"**
5. Scroll to find these fields:
   - **"Application privacy policy link"** → `https://btakita.github.io/corky/reference/privacy-policy.html`
   - **"Application terms of service link"** → `https://btakita.github.io/corky/reference/terms-of-service.html`
6. Click **Save**

Should we add the logo? Make a 120x120 square logo for GCP.

**If no "Branding" tab exists:**
- Click the edit/pencil icon next to your app name
- Walk through the wizard — URLs are in step 1 ("App information")
- The fields are labeled "Application privacy policy link" and "Application terms of service link"

**Verify the URLs are live first:**
- Check `https://btakita.github.io/corky/reference/privacy-policy.html` loads in your browser
- If 404, check GitHub Actions tab on the corky repo — the mdBook deployment may still be in progress

After adding the URLs, the last Phase 1 step is submitting for verification.

### Re: Logo for GCP consent screen

**Corky already has a logo!** Found at `src/corky/assets/logo-128.png` — a corgi character icon, 128x128 pixels. GCP wants 120x120 minimum, so 128x128 works perfectly (it'll resize).

**To upload it to the consent screen:**
1. In the Branding tab (same place as privacy policy URLs)
2. Look for "App logo" or "Application logo" field
3. Upload `src/corky/assets/logo-128.png`
4. Save

You can also create a 120x120 version from the SVG source at `src/corky/assets/logo.svg` if you want exact dimensions, but 128x128 is fine.

How do I submit for Google verification?
<!-- /agent:exchange -->

<!-- agent:pending patch=replace -->
- [ ] Phase 1: Register public Google OAuth app
  - [x] Create GCP project (corky-490604)
  - [x] Enable Gmail API
  - [x] Configure OAuth consent screen (scopes added)
  - [x] Create OAuth client credentials (in pass)
  - [x] Draft privacy policy & terms of service
  - [x] Push docs to corky repo (deploying via GitHub Pages)
  - [x] Enter URLs in consent screen
  - [ ] Submit for Google verification
- [ ] Phase 2: Built-in default credentials in corky
- [ ] Phase 3: `corky doctor` command
- [ ] Phase 4: Self-serve documentation
<!-- /agent:pending -->
