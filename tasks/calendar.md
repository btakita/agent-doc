---
agent_doc_session: a2b27649-1ed4-4962-a92e-2506a7953fe2
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
I have 2 google accounts with a calendar.
How do I share a calendar from 1 google account to another?

**Re: Sharing a Google Calendar between accounts**

To share a calendar from Account A to Account B:

1. **In Account A** (the calendar owner):
   - Go to calendar.google.com
   - Hover over the calendar in the left sidebar, click the three dots, then **Settings and sharing**
   - Scroll to **Share with specific people or groups**
   - Click **+ Add people and groups**
   - Enter Account B's email address
   - Set permission level:
     - **See only free/busy** — minimal
     - **See all event details** — read-only
     - **Make changes to events** — read/write
     - **Make changes and manage sharing** — full control
   - Click **Send**

2. **In Account B** (the recipient):
   - Check email for the sharing invitation and click **Add this calendar**
   - Or: In Google Calendar, click **+** next to "Other calendars" then **Subscribe to calendar** and enter Account A's email

The shared calendar will appear under "Other calendars" in Account B's sidebar.
<!-- agent:boundary:85a50126 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
<!-- /agent:pending -->
