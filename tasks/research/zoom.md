---
agent_doc_session: 35da9dd6-f793-444c-a8ca-d1bfb20b6552
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
❯ rob-ross is seeing the following when I share my screen on zoom.
❯ How can I properly share my screen, without the overlay, so Rob can see my screen.
❯ ![img.png](img.png)
### Re: Zoom screen sharing overlay (HEAD)

Looking at the screenshot, Rob sees mostly a dark screen with a green border. This is the classic "sharing your whole screen while Zoom is on it" problem.

**What's happening:** You're sharing your entire screen, and Zoom's own window is visible in that share. Rob sees a mostly empty/dark view because the shared content is the Zoom UI itself sitting on top of your desktop.

**Fix options:**

1. **Share a specific window** (best): In Zoom → "Share Screen" → switch to the **"Window"** tab → select the specific app (browser, terminal, IDE) you want Rob to see. This excludes Zoom itself.

2. **Share only the non-Zoom monitor**: If you have two monitors, share the one that Zoom is not open on.

3. **Advanced > Portion of Screen**: Drag a rectangle over just the content area, avoiding the Zoom window.

On Linux: the green border is Zoom's capture region indicator shown to you — Rob doesn't see the border itself. The dark area Rob sees is your desktop background behind the Zoom window.
<!-- agent:boundary:52ddc533 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
<!-- /agent:pending -->
