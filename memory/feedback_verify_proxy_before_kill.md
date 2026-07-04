---
name: Verify which process serves a URL before stopping anything
description: When multiple web servers are present (host Apache + Docker nginx + Docker Apache), confirm the actual routing topology before killing processes to free ports
type: feedback
originSessionId: 3454b7e9-664c-497d-8ac7-2886b912a598
---
Before stopping any web server or killing processes on ports 80/443, verify which process actually serves the target URL.

**Why:** On the MonsterRodHolders server (50.28.2.199), I assumed the Docker `app-nginx-1` container was the HTTPS reverse proxy for `50.28.2.199.nip.io`. It wasn't — the **host Apache** (`systemctl httpd`) was the actual proxy. When I ran `pkill -9 httpd` to free port 80 for certbot, I took down the live site. The Docker nginx had been broken in a restart loop for unrelated reasons (missing certs for monsterrodholders.com), masking the real topology.

**How to apply:**
1. Before stopping any web server, run `lsof -i :80 -i :443` or `ss -tlnp | grep -E ':80|:443'` to see what's actually bound to the ports
2. Check `curl -I <url>` and look at the `Server:` header — matches what you're about to stop?
3. If you see multiple web servers (Docker containers AND host processes), map the routing before touching anything
4. For certbot on a server with port 80 in use, prefer `--webroot` (drops a file into the existing webroot) or `--apache`/`--nginx` plugins over `--standalone` (which requires stopping the existing server)
5. If you must stop something to free a port, stop ONE thing at a time and verify you still understand the topology before proceeding
