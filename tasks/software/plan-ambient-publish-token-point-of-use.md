# Point-of-use publish credentials (`#lzmiseusepatch`)

Status: prepared and statically verified only. Do not apply from an agent queue.
The operator owns the quiet-window application and independent credential
rotations.

## Scoped design

Keep `.mise.toml`'s `_.source = ".env"`. Removing that line would disable every
root `.env` assignment, including unrelated service credentials and ordinary
settings whose consumers are outside this task. Instead, comment only the six
publish aliases in the ignored root `.env`. Mise will continue to load the
remaining environment while no longer evaluating or exporting a Cargo, npm, or
PyPI publishing credential.

The root and `src/lazily-js` `.env` files are ignored operator files, so their
edits are local operations rather than repository commits. The tracked call-site
patches resolve a credential only inside the publisher recipe/process. Every
lookup checks both the `passage` exit status and a non-empty result before the
publisher starts. The lookup runs after `set +x`, and no proof command prints a
credential value.

## Ready-to-apply patches

Each code block is a complete zero-context unified diff relative to the
repository named in its heading. First verify the preimage hash below, save each
block verbatim, and run `git apply --unidiff-zero --check` in that repository.
Apply only after all checks pass and the ignored-file edits have been reviewed.
The checks do not evaluate `passage`.

| Preimage path | SHA-256 at preparation |
|---|---|
| `.env` | `925444427b8220f0a8f4456a0c0419e420a3d8e86c13f7f34b4c52365614b79b` |
| `Makefile` | `f8bdf25baefbcf421b3dd8ff9626061f3632de9fd0ca5416482949ce8de5f21e` |
| `src/module-harness/Makefile` | `9118a00b4b0ceeebc21edce85c79896708192e1481c56e08dd052eef8edae718` |
| `src/corky/Makefile` | `b916ec15aff54522381a9ae3684d872391caa9b8aa1d169de4914ce58dc1bfe3` |
| `src/agent-doc/Makefile` | `d922c0547cd68b07cf577157295b8648d0e07bb7197483fbd1a961dfe963205a` |
| `src/lazily-py/Makefile` | `9800bc245c846bea80d6f5de1bd73c6bdd9226482c876dfdbc46644394baeb54` |
| `src/lazily-js/.env` | `b2aa4db4666435d210c0a711739b0c7a55637431b8236b7566d3fd8e489f0023` |
| `src/ctx-core-dev/lib/monorepo/bin/publish--force.sh` | `ce5ebd5f0f431b30b534a1e12a0535608c5836f9706574f8613ee8506d9246d1` |

### Superproject root

The `.env` hunk is intentionally local/ignored. `.mise.toml` is intentionally
unchanged.

```diff
diff --git a/.env b/.env
--- a/.env
+++ b/.env
@@ -1,3 +1,4 @@
-PYPI_PUBLISH_TOKEN="$(passage btak/PYPI_PUBLISH_TOKEN)"
-UV_PUBLISH_TOKEN="$(passage btak/PYPI_PUBLISH_TOKEN)"
-MATURIN_PYPI_TOKEN="$(passage btak/PYPI_PUBLISH_TOKEN)"
+# Publish credential references (documentation only; resolve at point of use):
+# PYPI_PUBLISH_TOKEN="$(passage btak/PYPI_PUBLISH_TOKEN)"
+# UV_PUBLISH_TOKEN="$(passage btak/PYPI_PUBLISH_TOKEN)"
+# MATURIN_PYPI_TOKEN="$(passage btak/PYPI_PUBLISH_TOKEN)"
@@ -5 +6 @@
-CARGO_REGISTRY_TOKEN="$(passage btak/CARGO_REGISTRY_TOKEN)"
+# CARGO_REGISTRY_TOKEN="$(passage btak/CARGO_REGISTRY_TOKEN)"
@@ -10 +11 @@
-NPM_PUBLISH_TOKEN="$(passage btak/NPM_PUBLISH_TOKEN)"
+# NPM_PUBLISH_TOKEN="$(passage btak/NPM_PUBLISH_TOKEN)"
@@ -23 +24 @@
-NPM_TOKEN="$(passage btak/GRANULAR_NPM_TOKEN)"
+# NPM_TOKEN="$(passage btak/GRANULAR_NPM_TOKEN)"
diff --git a/Makefile b/Makefile
--- a/Makefile
+++ b/Makefile
@@ -45 +45,6 @@
-	@cd src/instruction-files && cargo publish
+	@cd src/instruction-files && { \
+		set +x; \
+		token="$$(passage btak/CARGO_REGISTRY_TOKEN)" || { printf '%s\n' 'CARGO_REGISTRY_TOKEN lookup failed' >&2; exit 1; }; \
+		test -n "$$token" || { printf '%s\n' 'CARGO_REGISTRY_TOKEN lookup returned empty' >&2; exit 1; }; \
+		CARGO_REGISTRY_TOKEN="$$token" cargo publish; \
+	}
```

The independent root-path defect is fixed: both `publish` and `clean` now enter
`src/instruction-files`. `make -n publish` and `make -n clean` resolve that path,
the directory exists, and neither dry run contains the stale `cd
instruction-files` form. No publisher or cleaner was executed. The credential
hunk above is refreshed against this corrected preimage.

### `src/module-harness`

```diff
diff --git a/Makefile b/Makefile
--- a/Makefile
+++ b/Makefile
@@ -38 +38,4 @@
-	cargo publish
+	@set +x; \
+		token="$$(passage btak/CARGO_REGISTRY_TOKEN)" || { printf '%s\n' 'CARGO_REGISTRY_TOKEN lookup failed' >&2; exit 1; }; \
+		test -n "$$token" || { printf '%s\n' 'CARGO_REGISTRY_TOKEN lookup returned empty' >&2; exit 1; }; \
+		CARGO_REGISTRY_TOKEN="$$token" cargo publish
```

### `src/corky`

```diff
diff --git a/Makefile b/Makefile
--- a/Makefile
+++ b/Makefile
@@ -88,6 +88,9 @@
-	cargo publish -p corky-core
-	cargo publish -p corky-transcribe
-	cargo publish -p corky-google
-	cargo publish -p corky-social
-	cargo publish -p corky-mail
-	cargo publish -p corky
+	@set +x; \
+		token="$$(passage btak/CARGO_REGISTRY_TOKEN)" || { printf '%s\n' 'CARGO_REGISTRY_TOKEN lookup failed' >&2; exit 1; }; \
+		test -n "$$token" || { printf '%s\n' 'CARGO_REGISTRY_TOKEN lookup returned empty' >&2; exit 1; }; \
+		CARGO_REGISTRY_TOKEN="$$token" cargo publish -p corky-core && \
+		CARGO_REGISTRY_TOKEN="$$token" cargo publish -p corky-transcribe && \
+		CARGO_REGISTRY_TOKEN="$$token" cargo publish -p corky-google && \
+		CARGO_REGISTRY_TOKEN="$$token" cargo publish -p corky-social && \
+		CARGO_REGISTRY_TOKEN="$$token" cargo publish -p corky-mail && \
+		CARGO_REGISTRY_TOKEN="$$token" cargo publish -p corky
@@ -97 +100,4 @@
-	.venv/bin/maturin publish --skip-existing
+	@set +x; \
+		token="$$(passage btak/PYPI_PUBLISH_TOKEN)" || { printf '%s\n' 'PYPI_PUBLISH_TOKEN lookup failed' >&2; exit 1; }; \
+		test -n "$$token" || { printf '%s\n' 'PYPI_PUBLISH_TOKEN lookup returned empty' >&2; exit 1; }; \
+		MATURIN_PYPI_TOKEN="$$token" .venv/bin/maturin publish --skip-existing
```

### `src/agent-doc`

```diff
diff --git a/Makefile b/Makefile
--- a/Makefile
+++ b/Makefile
@@ -334 +334,4 @@
-	.venv/bin/python -m twine upload --skip-existing dist/*
+	@set +x; \
+		token="$$(passage btak/PYPI_PUBLISH_TOKEN)" || { printf '%s\n' 'PYPI_PUBLISH_TOKEN lookup failed' >&2; exit 1; }; \
+		test -n "$$token" || { printf '%s\n' 'PYPI_PUBLISH_TOKEN lookup returned empty' >&2; exit 1; }; \
+		TWINE_USERNAME=__token__ TWINE_PASSWORD="$$token" .venv/bin/python -m twine upload --skip-existing dist/*
```

### `src/lazily-py`

The TestPyPI target deliberately receives no production credential.

```diff
diff --git a/Makefile b/Makefile
--- a/Makefile
+++ b/Makefile
@@ -140 +140,4 @@
-	python -m twine upload dist/*
+	@set +x; \
+		token="$$(passage btak/PYPI_PUBLISH_TOKEN)" || { printf '%s\n' 'PYPI_PUBLISH_TOKEN lookup failed' >&2; exit 1; }; \
+		test -n "$$token" || { printf '%s\n' 'PYPI_PUBLISH_TOKEN lookup returned empty' >&2; exit 1; }; \
+		TWINE_USERNAME=__token__ TWINE_PASSWORD="$$token" python -m twine upload dist/*
```

### `src/lazily-js`

This `.env` hunk is intentionally local/ignored.

```diff
diff --git a/.env b/.env
--- a/.env
+++ b/.env
@@ -1 +1 @@
-NPM_TOKEN="$(passage btak/GRANULAR_NPM_TOKEN)"
+# NPM_TOKEN="$(passage btak/GRANULAR_NPM_TOKEN)"  # documentation only
```

### `src/ctx-core-dev`

```diff
diff --git a/lib/monorepo/bin/publish--force.sh b/lib/monorepo/bin/publish--force.sh
--- a/lib/monorepo/bin/publish--force.sh
+++ b/lib/monorepo/bin/publish--force.sh
@@ -61,5 +61,16 @@
-	bun publish --tag latest --access public
-	if [ $? -ne 0 ]; then
-		not_published 'Publish failed'
-		exit 1
-	fi
+	set +x
+	if ! publish_token="$(passage btak/GRANULAR_NPM_TOKEN)"; then
+		printf '%s\n' 'NPM_CONFIG_TOKEN lookup failed' >&2
+		exit 1
+	fi
+	if [ -z "$publish_token" ]; then
+		printf '%s\n' 'NPM_CONFIG_TOKEN lookup returned empty' >&2
+		exit 1
+	fi
+	NPM_CONFIG_TOKEN="$publish_token" bun publish --tag latest --access public
+	publish_status=$?
+	unset publish_token
+	if [ "$publish_status" -ne 0 ]; then
+		not_published 'Publish failed'
+		exit 1
+	fi
```

## Call-site and automation inventory

The live non-CI production-publish scan resolves to exactly these consumers:

| Surface | Local production consumers | Point-of-use variable |
|---|---|---|
| crates.io | root `Makefile`; `src/module-harness/Makefile`; six commands in `src/corky/Makefile` | `CARGO_REGISTRY_TOKEN` |
| PyPI | `src/agent-doc/Makefile` Twine; `src/lazily-py/Makefile` production Twine; `src/corky/Makefile` Maturin | `TWINE_PASSWORD` or `MATURIN_PYPI_TOKEN` |
| npm | `src/ctx-core-dev/lib/monorepo/bin/publish--force.sh` | `NPM_CONFIG_TOKEN` |
| TestPyPI | `src/lazily-py/Makefile` | deliberately no production credential |

The remaining live publish commands are workflow-owned. Each receives a
repository secret or an OIDC job output: Cargo workflows use
`${{ secrets.CARGO_REGISTRY_TOKEN }}` or `rust-lang/crates-io-auth-action`; npm
workflows use `${{ secrets.NPM_TOKEN }}` or trusted publishing; PyPI workflows
use `${{ secrets.PYPI_PUBLISH_TOKEN }}` or trusted publishing. No workflow
sources the root `.env`. Local tagpath and tsift publisher commands outside this
table are dry-run/package checks, not production publishes.

`NPM_CONFIG_TOKEN` is the Bun-compatible token variable. Token presence does not
prove npm authorization: the replacement token must be write-capable and satisfy
the registry's current 2FA/bypass policy before an operator attempts a real
publish.

## Static and non-publishing proof

Before application:

1. Extract each patch block and run `git apply --unidiff-zero --check` in its
   named repository.
   For the ignored `.env` files, also inspect the exact before/after lines; do
   not stage or commit them.
2. Re-run the production-call inventory and verify it has exactly the call sites
   in the table. Re-run the workflow scan and verify every production publisher
   has either a `${{ secrets.* }}` environment or an OIDC authentication step.
3. Reject any patch that removes `.mise.toml`'s `_.source = ".env"`, changes an
   unrelated `.env` assignment, evaluates `passage`, or modifies TestPyPI.

Latest preparation proof (2026-09-26): the call-site/workflow inventory still
matches the table above; all seven fenced patch sets pass
`git apply --unidiff-zero --check` in their named repositories; every recorded
preimage SHA-256 matches; and root `publish`/`clean` path checks pass under
`make -n`. The root `make check` suite also passes. These checks neither
evaluated `passage` nor invoked a publisher.

Steps 1-5 below are now a committed runnable target rather than a manual
procedure: `make publish-credential-proof` (implemented by
`scripts/proof-point-of-use-publish-credentials.py`). It recomputes every
preimage hash, runs `git apply --unidiff-zero --check` per patch, rebuilds each
patched file in a scratch checkout, and exercises all eight production call
sites in success, failure, and empty modes against fake `passage`, `cargo`,
`maturin`, `bun`, and `python -m twine` recorders. It resolves no credential and
invokes no real publisher, so it is safe to run before application; it is also
what caught this document's own `src/agent-doc/Makefile` preimage drift. Its
`--self-test` negative controls must pass first, so a proof whose assertions
cannot fail never reports success.

After operator application, use fake executables only—never a real publisher:

1. Put a fake `passage` first on `PATH`. It returns a fixed nonsecret sentinel,
   records only the requested key name, and supports explicit failure and empty
   modes.
2. Intercept `cargo`, `maturin`, and `bun` with fake executables. For Twine,
   intercept the exact Python invocation: place a fake `twine/__main__.py` on a
   temporary `PYTHONPATH` for `python -m twine`; for agent-doc's absolute venv
   Python, use a temporary copy/wrapper of that interpreter path in an isolated
   checkout. Do not assume a `twine` executable on `PATH` intercepts
   `python -m twine`.
3. In success mode, assert the publisher child sees exactly the expected
   variable with the sentinel while its parent has none of the six root aliases
   or the child-only `NPM_CONFIG_TOKEN`. Assert TestPyPI sees no production alias
   or `TWINE_PASSWORD`.
4. In failure and empty modes, assert every recipe exits nonzero and no fake
   publisher is invoked. This proves the lookup fails closed rather than
   falling back to a credential store or interactive prompt.
5. Start a fresh project shell and check names only:

   ```sh
   for name in PYPI_PUBLISH_TOKEN UV_PUBLISH_TOKEN MATURIN_PYPI_TOKEN CARGO_REGISTRY_TOKEN NPM_PUBLISH_TOKEN NPM_TOKEN NPM_CONFIG_TOKEN; do
     if env | cut -d= -f1 | grep -Fxq "$name"; then
       printf 'unexpected ambient variable: %s\n' "$name" >&2
       exit 1
     fi
   done
   ```

Old shells and IDE processes retain inherited values and are not evidence for
or against the fresh-shell result.

## Operator-only application and rotation

1. Choose a quiet window and close or stop long-running shells/IDE tasks rooted
   in this checkout.
2. Confirm the already-corrected root `src/instruction-files` path still passes
   the non-publishing dry-run checks.
3. Run all patch checks, then apply the ignored-file edits and per-repository
   tracked patches. Record each repository commit ID; this is intentionally not
   one atomic superproject commit.
4. Run normal non-publish checks plus the fake-publisher proof above. Do not
   perform a real publish merely to test scoping.
5. Rotate the Cargo, granular npm, legacy npm-publish (if retained), and PyPI
   credentials in their registries and in passage. For npm, create a token with
   the required write and 2FA-bypass policy or migrate the remaining local path
   to trusted publishing.
6. Close every process that predates application/rotation. Verify absence from
   fresh shells, then verify the next authorized release through its registry
   receipt.

## Rollback

Rollback is asymmetric: never uncomment the publish aliases or restore ambient
publish credentials. Revert or repair an individual tracked call-site commit,
then use an explicit one-command credential assignment for any emergency
publish. Preserve the unrelated `.mise.toml`/`.env` environment. Credential
rotation is irreversible; a reverted code commit does not restore a rotated
credential.
