---
agent_doc_session: 8a5e0e07-d470-4965-9ed4-1f8db8bbf465
agent_doc_format: template
agent_doc_write: crdt
links:
  - ./judith-krug.md
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->

### Re: Robert Waldman Contact Created

Created `mail/contacts/robert-waldman/AGENTS.md` (with `CLAUDE.md` symlink).

**Contact summary:**
- **Email:** rwaldman100@gmail.com
- **Role:** Judith Krug's tech/marketing consultant
- **Location:** Texas (kids moved back to CA, may return)
- **Family connection:** His family taught Judith the mortgage business
- **Key asset:** Enterprise GoHighLevel access (free sub-account for Judith)
- **Tools:** Notebook LM, Otter, Firefly, Read AI, Gemini
- **Content:** Wrote the existing equityfundingsource.com website copy

Sources used: March 4 three-way call transcript + diarized notes, email CC headers from proposal/kickoff/onboarding emails.

**Note:** Phone number was never shared. Action item #1 from March 4 meeting ("Judith to send Robert's email + phone") is still pending for the phone number.

Use corky to sync my Robert Waldman contact with https://contacts.google.com/person/c2907734623862917833
### Re: Google Contacts Sync

Added `robert-waldman` to `mail/.corky.toml` and pushed to Google Contacts via `corky contact push`.

**Result:** Created new Google Contact — `people/c5045079896432711148`

**Duplicate warning:** You provided Google Contacts URL with person ID `c2907734623862917833`, but corky created a new one (`c5045079896432711148`). Robert Waldman already exists in your Google Contacts. You may want to merge the duplicate in Google Contacts.
Yes

Please create a bi-directional link between this document + judith-krug.md
### Re: Bi-directional Links Created

- `tasks/professional/robert-waldman.md` → links to `./judith-krug.md`
- `tasks/professional/judith-krug.md` → links to `./robert-waldman.md`

Both frontmatter `links` arrays updated.
Implement. Does corky need a feature to perform this action?
- [ ] Merge duplicate Robert Waldman in Google Contacts (c2907734623862917833 vs c5045079896432711148)
### Re: Google Contacts Merge (HEAD)

Corky doesn't have a merge or delete feature for Google Contacts yet. It supports `createContact` and `updateContact` but not `deleteContact`.

**Simplest fix:** Open Google Contacts in your browser, search "Robert Waldman", and merge the two entries. Google has a built-in "Merge & fix" feature that detects duplicates automatically.

**For corky:** A `corky contact delete` subcommand (calls `people.googleapis.com/v1/{resourceName}:deleteContact`) would be a small feature to add later. Not worth building just for this one merge.
<!-- agent:boundary:30d5a03b -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] Merge duplicate Robert Waldman in Google Contacts manually (browser)
- [ ] Get Robert Waldman's phone number (pending from Judith since March 4)
- [ ] Schedule Zoom call with Robert Waldman for deeper requirements
<!-- /agent:pending -->
