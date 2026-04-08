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
### Re: Google Contacts Merge

Corky doesn't have a merge or delete feature for Google Contacts yet. It supports `createContact` and `updateContact` but not `deleteContact`.

**Simplest fix:** Open Google Contacts in your browser, search "Robert Waldman", and merge the two entries. Google has a built-in "Merge & fix" feature that detects duplicates automatically.

**For corky:** A `corky contact delete` subcommand (calls `people.googleapis.com/v1/{resourceName}:deleteContact`) would be a small feature to add later. Not worth building just for this one merge.

Implement `corky contact delete`. Does the Google contact api support merging contacts?
Please draft an email to robert-waldman notifying him that equityfundingsource.com is down.
Let him know the current status with my work for Judith. I will attempt to recover the site.
### Re: Email Draft & Corky Contact Delete

#### Email to Robert Waldman

> Hi Robert,
>
> Just a heads up — equityfundingsource.com appears to be down. I noticed it while preparing for our call with Judith.
>
> I'm going to work on recovering the site and migrating it to Cloudflare today. Since the current host has been unreliable (the "control freak" situation Judith mentioned), this is a good opportunity to get it moved over to infrastructure we control.
>
> I'll keep you posted on progress. If Judith asks about it, you can let her know I'm already on it.
>
> Brian

**Draft file:** `mail/drafts/20260401-robert-waldman-website-down.md`

Link to https://web.archive.org/web/20260207050228/https://www.equityfundingsource.com/
Note that the CSS does not layout correctly and some of the images are missing.
Ask Robert if he has any of the missing assets for the website. I will work on the css layout.

#### Google People API — Merge Support

**No native merge endpoint.** To merge contacts:
1. Copy data from one contact to the other (`updateContact`)
2. Delete the duplicate (`deleteContact`: `DELETE /v1/{resourceName}:deleteContact`)

Implementing `corky contact delete` now.

Robert Waldman contacts are merged.
### Re: Email Draft & corky contact delete

#### Email Draft Created

**File:** `mail/drafts/20260401-robert-waldman-website-down.md`
- **To:** rwaldman100@gmail.com
- **Subject:** Equity Funding Source website is down
- Notifies Robert the site is down, Brian is recovering/migrating to Cloudflare

Ready to send via `corky draft push` when you approve.

#### corky contact delete — Implemented

Added `corky contact delete <RESOURCE_NAME>...` command.
- Accepts `people/c123` or just `c123` format
- Calls `DELETE /v1/{resourceName}:deleteContact` on Google People API
- Files: `src/contact/delete.rs` (new), `cli.rs`, `main.rs`, `contact/mod.rs` (updated)
- Build: passing (`make build`)

Tried deleting the duplicate (`c5045079896432711148`) — got 404, meaning it was already removed (you may have already merged it in Google Contacts).

#### Google People API — No Native Merge

The API has no merge endpoint. To merge: copy data from one contact to the other (`updateContact`), then delete the source (`deleteContact`). The Google Contacts web UI "Merge & fix" is still the easiest path for one-off merges.
### Re: Email Draft Updated

Updated `mail/drafts/20260401-robert-waldman-website-down.md`:
- Added Wayback Machine archive link
- Noted CSS layout issues and missing images
- Asking Robert for original assets (images, logos)
- You'll handle the CSS layout fix

Ready to send via `corky draft push` when you approve.

Ask Robert if he knows about this.
I don't want to alarm Judith and I know that she is busy.
### Re: Email Updated — Ask Robert, Don't Alarm Judith

Updated the draft to:
- Ask Robert if he knew the site was down and what happened with the hosting
- Explicitly note: don't alarm Judith, just have it handled before the call
- If she asks, Robert can say Brian's on it

Ready to send when you approve.
<!-- agent:boundary:750c9c2b -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] Send email draft to Robert Waldman (website down + asset request) — awaiting approval
- [ ] Fix CSS layout from Wayback Machine recovery
- [ ] Recover missing images for equityfundingsource.com
- [ ] Migrate site to Cloudflare
- [ ] Get Robert Waldman's phone number (pending from Judith since March 4)
- [ ] Schedule Zoom call with Robert Waldman for deeper requirements
<!-- /agent:pending -->
