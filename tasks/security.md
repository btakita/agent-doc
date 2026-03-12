---
agent_doc_session: f59ded2b-e4f4-45b6-9f83-b79c413b80ec
tmux_session: '0'
agent_doc_mode: stream
---

If I use pass in all of my .env files, how do I transfer the passwords to another machine if needed?

<!-- agent:exchange -->
### Re: Transferring `pass` to Another Machine

`pass` stores everything in `~/.password-store/` as GPG-encrypted files, typically in a git repo. Two things to transfer: the store and your GPG key.

#### 1. Transfer the password store

**Via git (recommended — pass already uses git):**
```bash
# Source machine:
pass git remote add origin git@github.com:btakita/password-store.git
pass git push -u origin main

# New machine:
git clone git@github.com:btakita/password-store.git ~/.password-store
```

**Via rsync:**
```bash
rsync -av ~/.password-store/ newmachine:~/.password-store/
```

#### 2. Transfer your GPG private key

This is the critical part — without it, the encrypted files are useless:

```bash
# Export from source:
gpg --export-secret-keys --armor YOUR_KEY_ID > private-key.asc

# Transfer securely (scp, USB drive, etc.):
scp private-key.asc newmachine:~/

# Import on new machine:
gpg --import private-key.asc
shred -u private-key.asc  # securely delete the export

# Trust it:
gpg --edit-key YOUR_KEY_ID  # then: trust > 5 (ultimate) > quit
```

#### 3. For .env files: regenerate, don't transfer

The .env files themselves shouldn't be transferred — regenerate them from `pass`:

```bash
#!/bin/bash
# gen-env.sh — regenerate .env from pass
echo "API_KEY=$(pass show project/api-key)" > .env
echo "DB_PASSWORD=$(pass show project/db-password)" >> .env
```

This way secrets never sit in plaintext on disk. The password store + GPG key is all you need on any machine.

**Find your key ID:** `gpg --list-secret-keys --keyid-format long`

How do I access pass from another machine? Do I need the password for the gpg store?
Can I merge in a gpg store from another machine into the new machine?

<!-- /agent:exchange -->
