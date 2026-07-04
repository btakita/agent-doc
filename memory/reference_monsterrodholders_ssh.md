---
name: monsterrodholders SSH access
description: SSH details + deploy path for the monsterrodholders-dev WordPress stack running on 50.28.2.199
type: reference
originSessionId: 063e81d1-2a90-44ec-996b-c931fd8c96c5
---
**Host:** `50.28.2.199` (public Linode/metalseed.net, nip.io hostname `50.28.2.199.nip.io`)

**SSH:** `ssh monsterrodholders-server` (alias in `~/.ssh/config` → `root@50.28.2.199` with `~/.ssh/id_ed25519`). Never use `user@` — the account is `root`. If a command literally writes `user@50.28.2.199`, that's a placeholder, not the literal username.

**Repo on server:** `/opt/monsterrodholders/app` (git checkout of `btakita/monsterrodholders-dev`, remote alias `github-monsterrodholders`).

**Deploy:**
```bash
ssh monsterrodholders-server "cd /opt/monsterrodholders/app && git pull && ./scripts/deploy.sh"
```

**When compose config changes** (new volume, new service), plain `up -d` may skip container recreation. Force it:
```bash
docker compose up -d --force-recreate wordpress
```

**Verify after deploy:**
```bash
curl -sk https://50.28.2.199.nip.io/shop/ | grep -c 'mrh-product-grid'   # expect 1 for the mu-plugin shop-grid CSS
```
