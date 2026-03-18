---
agent_doc_session: 6b032495-88f6-4129-b57a-02ffb785d10d
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
---


# Corky Public Google App

Publish a verified Google OAuth app for corky so users don't need to create their own credentials.

## Requirements

- Register corky as a Google OAuth app
- Request `gmail.readonly` scope (requires Google security review)
- Publish privacy policy and terms of service
- Domain verification for `corky` brand
- Ship default client credentials in corky binary (or fetch from a config endpoint)
- Users opt in by setting `provider = "gmail-api"` without `[gmail]` credentials — falls back to published app

## Notes

- Self-hosted credentials remain the default (no trust dependency)
- Published app is an optional convenience for users who don't want to create their own GCP project
- Google security review for `gmail.readonly` can take weeks
<!-- agent:status mode=replace -->
<!-- /agent:status -->

<!-- agent:exchange mode=append -->
Does Google Compute Cloud have the equivalent of AWS CDK?
Can we create a config file that syncs a Google Compute Cloud app?

### Re: GCP Infrastructure-as-Code (CDK equivalent)

**Yes — GCP has several IaC options:**

1. **Terraform (with Google provider)** — HCL-based, declarative. Most widely used for GCP IaC. Mature Google provider.
2. **Pulumi** — Write infra in TypeScript/Python/Go. Closest to AWS CDK in spirit (real languages, not YAML).
3. **Google Cloud Deployment Manager** — GCP's native IaC. YAML/Jinja2/Python templates. Closer to CloudFormation than CDK.
4. **Direct API/gcloud scripting** — No state management, but works for simple setups.

### Re: Config file to sync a Google Cloud OAuth app

**Yes, partially.** Terraform can declare the OAuth app resources:

```hcl
resource "google_project" "corky" {
  name       = "corky"
  project_id = "corky-mail"
  org_id     = var.org_id
}

resource "google_project_service" "gmail" {
  project = google_project.corky.project_id
  service = "gmail.googleapis.com"
}

resource "google_iap_brand" "corky" {
  project           = google_project.corky.project_id
  support_email     = var.support_email
  application_title = "Corky"
}

resource "google_iap_client" "corky_oauth" {
  brand        = google_iap_brand.corky.name
  display_name = "Corky Desktop Client"
}
```

**Caveat:** The OAuth consent screen (scopes, privacy policy URL, verification status) is only partially automatable via API. The Google security review for `gmail.readonly` is a manual step regardless — Terraform can provision the project and credentials, but verification still goes through the GCP Console.

**Recommendation:** Terraform is the pragmatic choice — mature GCP provider, state management, and corky doesn't need CDK-level imperative flexibility.

This would require the person to install Terraform?
How would this be used?

### Re: Would users need Terraform? / How would this be used?

**No, end users of corky would NOT need Terraform.** Terraform is only for *you* (the corky maintainer) to provision the GCP project. Here's the end-to-end flow:

1. **Maintainer (you):** Use Terraform to create the GCP project, OAuth consent screen, and client credentials. One-time setup + occasional updates.
2. **Build pipeline:** The OAuth `client_id` gets embedded in the corky binary at build time (or served from a config endpoint). For "installed application" OAuth flows, the client_id is inherently public — security comes from the OAuth redirect and user consent, not from hiding the client ID.
3. **End user:** Sets `provider = "gmail-api"` in their corky config without custom `[gmail]` credentials. Corky uses the built-in public app credentials. User clicks through the OAuth consent screen in their browser — no GCP project, no Terraform, no developer setup.

**Key insight:** This is the same model used by Thunderbird, offlineimap, and other desktop Gmail clients. The `client_id` ships with the binary; the `client_secret` for installed apps is also considered public per Google's own docs.

So Terraform is purely maintainer-side infrastructure management. Users get a seamless "just log in" experience.

Only if they use the public Corky app. I need to set that up and register it in the public store. And it needs approval.

I also want to provide a self-serve solution for people to manage their own infrastructure.

### Re: Two tracks — public app + self-serve infrastructure

Right, so there are two parallel tracks:

**Track 1: Public Corky App (you manage)**
- Register OAuth app in GCP Console
- Submit for Google security review (`gmail.readonly`)
- Publish privacy policy + terms of service
- Embed approved `client_id` in corky binary
- Users just authenticate — zero setup

The `client_id` can also be configurable.

**Track 2: Self-Serve (user manages their own GCP project)**
Options for making self-serve easy:

1. **`corky init` CLI wizard** — Shell out to `gcloud` commands to walk users through creating their own OAuth app:
   ```
   corky init gmail
   # Opens browser → Create Project → Enable Gmail API
   # Creates OAuth consent screen + credentials
   # Writes client_id/client_secret to ~/.config/corky/gmail.toml
   ```
   Most user-friendly — no Terraform, just `gcloud` CLI.

Good. But we may also support other providers. Such as AWS, Azure, etc.

2. **Bundled Terraform module** — Ship `terraform/` in the corky repo. Users run `terraform apply`. Reproducible but higher barrier.

3. **Guided docs + config template** — Step-by-step guide with screenshots. User creates GCP project manually, pastes credentials into `corky.toml`. Lowest dependency, most manual.

**Recommendation:** Start with option 3 (docs + config template) — works today, no new code. Build toward option 1 (`corky init`) as a future convenience. Terraform module is nice-to-have for power users.

Option 3 sounds good as long as it is in sync. AWS CDK is nice because it enforces the synced configuration...even if it does require care to not wipe out production data.

The two tracks are independent — ship self-serve docs immediately while Google security review for the public app is pending (can take weeks).

### Re: Configurable client_id, multi-cloud, and sync concerns

**Configurable `client_id`:** Yes — the config should support overriding the public app credentials:

```toml
[gmail]
client_id = "user-provided-id.apps.googleusercontent.com"
client_secret = "user-provided-secret"
```

When present, corky uses these instead of the built-in defaults. The public app is just the fallback.

**Multi-cloud providers:** The `corky init` pattern generalizes:

- `corky init gmail` → GCP OAuth setup
- `corky init outlook` → Azure AD app registration
- `corky init aws-ses` → AWS SES/WorkMail credentials

Each cloud has its own CLI (`gcloud`, `az`, `aws`) and OAuth/API model, but the guided setup → write credentials pattern is the same.

**Sync concern:** You're right that plain docs drift. The CDK appeal is declarative state enforcement. For corky's case though, the sync problem is narrow:

- The "infrastructure" is just an OAuth app + API enablement — one-time setup, rarely changes
- Unlike full cloud deployments (lambdas, databases, networking), there's minimal ongoing drift
- The main sync point is "does `client_id` match your OAuth app?" — corky validates that at runtime when auth fails

For CDK-style enforcement without requiring users to learn IaC, consider a **`corky doctor`** subcommand:

```
corky doctor gmail
# ✓ OAuth app exists and is valid
# ✓ gmail.readonly scope authorized
# ✓ Token not expired
# ✗ Refresh token missing — run `corky auth gmail`
```

This gives runtime sync validation — the "enforcement" happens when you run `corky doctor`, not via infrastructure state files. Lighter weight than Pulumi/Terraform, and users don't need to install anything extra.

`corky doctor` sounds good.
Should it be one or both?:
- `corky doctor gmail`
- `corky gmail doctor`
<!-- /agent:exchange -->
