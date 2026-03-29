---
agent_doc_session: 5fb15dd1-10ba-4aa0-93aa-6c835bdbdc6b
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
Inspected Handshake AI form page — no downloadable video found, this is a video submission form
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
<!-- agent:boundary:ba3acbcb -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
<!-- /agent:pending -->
