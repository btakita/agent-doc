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
  - [ ] Host privacy policy & ToS at public URLs
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
<!-- /agent:exchange -->

<!-- agent:pending patch=replace -->
- [ ] Phase 1: Register public Google OAuth app
  - [x] Create GCP project (corky-490604)
  - [ ] Enable Gmail API
  - [x] Configure OAuth consent screen (scopes added)
  - [x] Create OAuth client credentials (in pass)
  - [x] Draft privacy policy & terms of service
  - [ ] Host privacy policy & ToS at public URLs
  - [ ] Submit for Google verification
- [ ] Phase 2: Built-in default credentials in corky
- [ ] Phase 3: `corky doctor` command
- [ ] Phase 4: Self-serve documentation
<!-- /agent:pending -->
