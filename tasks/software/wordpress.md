---
agent_doc_session: c547aab7-46a1-4af4-8e56-54e6261feceb
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
In WordPress, what is the difference between an Admin and a Super Admin.

Tell me about Liquid Web Wordpress Dedicated.
### Re: Admin vs Super Admin in WordPress

**Admin** is the highest role within a single WordPress site. An Admin can:
- Install/manage plugins and themes
- Add/remove users
- Edit all content
- Manage settings, menus, widgets

**Super Admin** only exists in **WordPress Multisite** (a network of WordPress sites managed from one installation). A Super Admin can:
- Create/delete sites in the network
- Install plugins and themes **network-wide**
- Manage all sites' users and settings
- Access the Network Admin dashboard
- Override per-site Admin decisions

Key distinction: An Admin controls one site. A Super Admin controls the entire multisite network.

### Re: Liquid Web WordPress Dedicated

Liquid Web WordPress Dedicated is a managed WordPress hosting platform by Liquid Web, aimed at businesses and agencies needing high performance and hands-off server management.

**Plans & Pricing:**

| Plan | Sites | Storage | Bandwidth | Price |
|------|-------|---------|-----------|-------|
| Personal | 1 | 20 GB SSD | 5 TB | $29/mo |
| Freelance | Up to 4 | 40 GB SSD | 5 TB | $69/mo |
| Professional | Up to 10 | 100 GB SSD | 5 TB | $99/mo |
| Business | Up to 25 | 150 GB SSD | 5 TB | $149/mo |

(Promotional pricing may start as low as $4-5/mo on annual plans.)

**Key Features:**
- 99.99% uptime guarantee
- NGINX + PHP 8+ + multi-layered caching + integrated CDN
- Staging environments and 30-day backups
- Free SSL certificates, malware monitoring, Solid Security Pro plugin
- US or Europe data center choice
- 24/7/365 WordPress expert support (59-second response guarantee)
- 30-day money-back guarantee

For dedicated servers specifically (bare metal, not shared WordPress), pricing starts at $149/mo.

**Sources:**
- [Liquid Web WordPress Hosting](https://www.liquidweb.com/wordpress-hosting/)
- [Liquid Web Managed WordPress](https://www.liquidweb.com/wordpress-hosting/managed-wordpress/)
- [Liquid Web Review - CyberNews](https://cybernews.com/best-web-hosting/liquid-web-review/)
  - US or Europe data center choice
  - 24/7/365 WordPress expert support (59-second response guarantee)
  - 30-day money-back guarantee

  For dedicated servers specifically (bare metal, not shared WordPress), pricing starts at $149/mo.

  Sources:
  - https://www.liquidweb.com/wordpress-hosting/
  - https://www.liquidweb.com/wordpress-hosting/managed-wordpress/
  - https://cybernews.com/best-web-hosting/liquid-web-review/

  Now writing back to the document.

  Read 1 file (ctrl+o to expand)

● Bash(agent-doc commit tasks/software/wordpress.md)
  ⎿  [perf] commit.staging (hash_object+update-index): 3ms
     [perf] commit.git_commit: 18ms
     [main ed0f7114] agent-doc(wordpress): 2026-04-03 12:21:39
     … +6 lines (ctrl+o to expand)

● Response written and committed. Both questions answered in the document — Admin vs Super Admin roles, and Liquid Web WordPress Dedicated plans/features/pricing.

✻ Baked for 1m 7s

  - US or Europe data center choice
  - 24/7/365 WordPress expert support (59-second response guarantee)
  - 30-day money-back guarantee

  For dedicated servers specifically (bare metal, not shared WordPress), pricing starts at $149/mo.

  Sources:
  - https://www.liquidweb.com/wordpress-hosting/
  - https://www.liquidweb.com/wordpress-hosting/managed-wordpress/
  - https://cybernews.com/best-web-hosting/liquid-web-review/

  Now writing back to the document.

  Read 1 file (ctrl+o to expand)

● Bash(agent-doc commit tasks/software/wordpress.md)
  ⎿  [perf] commit.staging (hash_object+update-index): 3ms
     [perf] commit.git_commit: 18ms
     [main ed0f7114] agent-doc(wordpress): 2026-04-03 12:21:39
     … +6 lines (ctrl+o to expand)

● Response written and committed. Both questions answered in the document — Admin vs Super Admin roles, and Liquid Web WordPress Dedicated plans/features/pricing.

✻ Baked for 1m 7s

How do I export a wordpress website?
### Re: How to Export a WordPress Website

There are a few methods depending on what you need:

#### Built-in WordPress Export (Tools > Export)

The simplest method — exports content as an XML file (WXR format):

1. Go to **Dashboard > Tools > Export**
2. Choose what to export: All content, Posts, Pages, Media, etc.
3. Click **Download Export File** — saves a `.xml` file
4. To import elsewhere: **Tools > Import > WordPress**, install the importer, upload the XML

**Limitation:** This only exports content (posts, pages, comments, users). It does NOT include plugins, themes, or database tables outside core WordPress.

#### Full Site Export (Database + Files)

For a complete migration (moving hosts, cloning site):

**Files:**
- Use FTP/SFTP or your host's file manager to download the entire `wp-content/` folder (themes, plugins, uploads)
- Also download `wp-config.php` (has DB credentials)

**Database:**
- Go to **phpMyAdmin** (via your host's cPanel/panel)
- Select your WordPress database → **Export** → Quick → Format: SQL → Go

**On the new host:**
1. Upload all files
2. Create a new database, import the SQL via phpMyAdmin
3. Update `wp-config.php` with the new DB name/user/password
4. If the domain changed, run search-replace on the DB (URLs are stored as absolute paths)

#### Plugin-based Migration (Easiest Full Migration)

**Duplicator** (free) or **All-in-One WP Migration** are the most popular:

- Install the plugin → create a package/backup
- Download the package file
- On the new site: install the same plugin, upload the package, follow the wizard

**All-in-One WP Migration** is the simplest — exports everything (DB + files) into a single `.wpress` file, handles search-replace for domain changes automatically. Free version is limited to 512MB imports; paid removes the limit.

❯ In wordpress, how do I see the site revisions?
**WordPress revisions** are per-post/page (not site-wide). Here's how to access them:

**In the Block Editor (Gutenberg):**
1. Open any post or page
2. In the right sidebar, click the **"Post"** tab
3. Scroll down to find **"Revisions"** — click it to see a count and open the revisions browser
4. Or: click the three-dot menu (⋮) at the top right → **"Revisions"**

**In the Classic Editor:**
- A **"Revisions"** meta box appears below the editor showing a count; click "Browse" to compare versions

**Revisions browser:**
- Slider to move through revisions
- Side-by-side diff showing what changed
- "Restore This Revision" button

**Note:** WordPress tracks revisions for posts, pages, and custom post types — not for plugin settings, theme options, or the database at large. If you want site-wide snapshots (including settings/media), that's what migration plugins like All-in-One WP Migration are for.

❯ Use my credentials to log into the api for https://monsterrodholders.com/
❯ Find out the most recent changes. The home page has major layout issues. Can you find out why?
### Re: monsterrodholders.com — Site Hacked, Homepage Down (HEAD)

I couldn't find WordPress credentials in the pass store. Here's what the public API revealed — the news is serious.

**The site has been hacked and the homepage is returning HTTP 500 (completely down).**

#### What I found (no credentials needed)

1. **Homepage → HTTP 500** — completely broken as of right now
2. **Home page modified today at 10:45 AM** — a suspicious edit that broke the WPBakery page structure
3. **Hidden spam link injected** into home page content:
   ```html
   <a href="https://eggsinc.se/" style="position: absolute;top:-9999px;left:-9999px">utländska casino</a>
   ```
   Swedish gambling site, hidden via CSS off-screen positioning (invisible to users, visible to search crawlers)
4. **Spam posts flooding the blog** — gambling/casino content in Russian and Azerbaijani (Aviator, 1xbet, Pin-Up Casino) published over the past 10+ days — classic SEO spam injection
5. **Structural breakage** — double nested `<section class="wpb-content-wrapper">` in the home page source; the attacker's edit corrupted the WPBakery shortcode structure, likely triggering the 500

#### Root cause

This is a **WordPress SEO spam hack** (a very common attack). The attacker gained WordPress admin access (or file-system access via a vulnerable plugin), then:
- Injected hidden outbound links to boost gambling sites in Google
- Published hundreds of spam posts for the same purpose
- Their edit to the home page inadvertently (or deliberately) broke the page structure, causing the 500

#### Immediate recovery steps

1. **Log into WP Admin** → Posts → find and bulk-delete all spam posts (gambling content in Russian/Azerbaijani)
2. **Home page → Revisions** → restore to a pre-April-9 revision (fixes the 500 and removes the injected link)
3. **Users → check for unauthorized admin accounts** → delete any unknown users
4. **Change all admin passwords**
5. **Install Wordfence** (free) → run a full malware scan, check for backdoors in `wp-content/uploads/` and theme files
6. **Check active plugins** for anything unfamiliar — attackers often install a hidden plugin as a backdoor

#### Credentials

I couldn't find WordPress credentials in `pass ls`. Can you point me to where they're stored, or log into the admin yourself at `https://monsterrodholders.com/wp-admin/`? Once you're in, I can guide the cleanup step by step.
<!-- agent:boundary:7a59133f -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
<!-- /agent:pending -->
