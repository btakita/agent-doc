---
agent_doc_session: 5fb15dd1-10ba-4aa0-93aa-6c835bdbdc6b
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
Clicked Start — Typeform modal opened, step 1 (name/email). No video visible yet.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
Use chromium-bridge to download the video on the https://ai.joinhandshake.com/fellow/forms/d6a5a8ad-257c-4b89-8c24-513653204040 Brave tab

### Re: Download video from Handshake AI

I inspected the page at `https://ai.joinhandshake.com/fellow/forms/d6a5a8ad-...`. This is a **video submission form**, not a page with a downloadable video.

**Page content:**
- Title: "Video Recording Interview Task"
- Instructions: "Please submit a short video response to complete this task. Make sure to click 'Start' and submit a response in the Typeform before typing 'yes' in the input below."
- A "Start" button (links to Typeform)
- A confirmation input field
- Warning: "Failing to submit video responses may result in project removal"

**No video elements found on the page** — no `<video>`, `<source>`, or video download links.

The "Start" button likely opens a Typeform where you record a video. Want me to click "Start" to see what's in the Typeform?
No...there is a video on the https://ai.joinhandshake.com/fellow/forms/d6a5a8ad-257c-4b89-8c24-513653204040 tab.
I just want you to download the video onto my local fs so I can transcribe it.

### Re: Video location

I clicked "Start" — it opened a Typeform modal overlay. The Typeform is on step 1 asking for name/email/last name. The video prompt likely appears on a later step after identity verification.

**Current state of the page:** Typeform modal is open with "Close X" button in top-right. No video visible yet — it's behind the form steps.

Could you clarify:
- Are you seeing a video playing right now on this tab that I'm not detecting?
- Or should I close this modal and look elsewhere on the page?
- Or do you want me to fill in the form to advance to the video step?
<!-- agent:boundary:62a1db67 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] Locate the video on the Handshake page (awaiting user clarification)
- [ ] Download video to local filesystem
- [ ] Transcribe video
<!-- /agent:pending -->
