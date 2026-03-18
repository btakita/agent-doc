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
<!-- /agent:exchange -->

<!-- agent:pending patch=replace -->
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
<!-- /agent:pending -->
