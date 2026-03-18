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
*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/aab8e49aea61417e3521b779b6c7cb27283e0b8f17d03f0d4285ff625b1a4920-20260318-053817.md`*

continue

### Session resumed after compaction

**Phase 1 progress:**
- [x] GCP project "corky" (corky-490604)
- [ ] Enable Gmail API
- [x] OAuth consent screen — scopes: `gmail.readonly`, `gmail.send`, `gmail.settings.basic`
- [x] OAuth client credentials — `pass corky/gcp/{client_id,client_secret}`
- [x] Privacy policy + ToS drafted — `src/corky/docs/reference/{privacy-policy,terms-of-service}.md`
- [ ] Host privacy policy & ToS at public URLs
- [ ] Submit for Google verification

**Files created this session:**
- `src/corky/docs/reference/privacy-policy.md` — covers all Gmail scopes, destructive actions, no data collection
- `src/corky/docs/reference/terms-of-service.md` — includes "Destructive Actions Warning" section, no-liability clause

**Key decisions made:**
- Verb-first CLI: `corky doctor gmail`
- Two tracks: public app (zero setup) + self-serve (own GCP project)
- Configurable `client_id` with public app as fallback
- `corky doctor` for runtime validation instead of IaC
- Credentials work pre-verification (unverified warning only)

**Next:** Enable Gmail API → host docs → submit for verification. Or jump to Phase 2 (embed credentials).

Finish Phase 1. Enable Gmail API. We can host the privacy policy & ToS at https://btakita.github.io/corky/
Should we create a github organization for corky instead?

### Re: Finish Phase 1, hosting, and GitHub org

**Enable Gmail API:** Manual step — GCP Console → APIs & Services → Library → "Gmail API" → Enable. Do this now if not done.

**Hosting at `btakita.github.io/corky/`:** Works well. URLs would be:
- `https://btakita.github.io/corky/reference/privacy-policy.html`
- `https://btakita.github.io/corky/reference/terms-of-service.html`

Requires GitHub Pages enabled on the corky repo (Settings → Pages). The mdBook docs structure is already there.

**GitHub organization?**

| | Personal (`btakita/corky`) | Org (`corkymail/corky`) |
|---|---|---|
| URL | `btakita.github.io/corky` | `corkymail.github.io` |
| Branding | Personal project feel | Professional/official |
| Migration | None | Transfer repo, update refs |
| Multi-maintainer | Via collaborators | Native org teams |
| Google verification | Fine | Slightly more legitimate |

**Recommendation:** Stay on `btakita/corky` for now — don't let org setup block Phase 1. GitHub auto-redirects if you transfer later. OAuth URLs can be updated post-verification.

**Remaining Phase 1 steps:**
1. Enable Gmail API (you, in console): How?
2. Enable GitHub Pages on corky repo + push docs: Already done
3. Enter privacy policy + ToS URLs in consent screen: we need to push to gh pages
4. Submit for verification

### Re: Enable Gmail API, push docs to GitHub Pages

**How to enable Gmail API:**
1. GCP Console → select project "corky" (corky-490604)
2. Left sidebar → "APIs & Services" → "Enabled APIs & services"
3. Click "+ ENABLE APIS AND SERVICES" at the top
4. Search "Gmail API" → click it → click "ENABLE": Enabled. Should I enable YouTube as well? Which one?

**Docs pushed to GitHub Pages:**
- Committed privacy policy, ToS, and updated SUMMARY.md in corky repo
- Pushed to `main` — the `mdbook.yml` workflow will auto-deploy to GitHub Pages
- URLs once deployed:
  - `https://btakita.github.io/corky/reference/privacy-policy.html`
  - `https://btakita.github.io/corky/reference/terms-of-service.html`

**After deployment completes** (check GitHub Actions tab), enter those URLs in the GCP consent screen:
- OAuth consent screen → App information → Privacy policy link + Terms of service link: I don't see App information

Then you can submit for verification.
<!-- /agent:exchange -->

<!-- agent:pending patch=replace -->
- [ ] Phase 1: Register public Google OAuth app
  - [x] Create GCP project (corky-490604)
  - [ ] Enable Gmail API
  - [x] Configure OAuth consent screen (scopes added)
  - [x] Create OAuth client credentials (in pass)
  - [x] Draft privacy policy & terms of service
  - [x] Push docs to corky repo (deploying via GitHub Pages)
  - [ ] Enter URLs in consent screen
  - [ ] Submit for Google verification
- [ ] Phase 2: Built-in default credentials in corky
- [ ] Phase 3: `corky doctor` command
- [ ] Phase 4: Self-serve documentation
<!-- /agent:pending -->
