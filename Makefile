.PHONY: build build-release release test sim-medium tmux-ci clippy check precommit timings install install-full install-hooks clean init-python wheel publish publish-crate publish-pypi bump-plugin lean

CPU_COUNT ?= $(shell nproc 2>/dev/null || getconf _NPROCESSORS_ONLN 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo 4)
TEST_THREADS ?= 2
TMUX_TEST_THREADS ?= 1
CARGO_CLEAN_ENV = env -u GIT_DIR -u GIT_INDEX_FILE -u GIT_WORK_TREE
NEXTEST_QUIET_FLAGS ?= --cargo-quiet --show-progress none --status-level fail --final-status-level fail --failure-output immediate-final --success-output never
LOCAL_INSTALL_PROFILE ?= release-local
LOCAL_INSTALL_TARGET_DIR ?= target/local-install
LOCAL_LINKER ?= $(shell if command -v mold >/dev/null 2>&1; then printf '%s' mold; elif command -v ld.lld >/dev/null 2>&1 || command -v lld >/dev/null 2>&1; then printf '%s' lld; fi)
LOCAL_RUSTFLAGS ?= $(if $(LOCAL_LINKER),-C link-arg=-fuse-ld=$(LOCAL_LINKER),)
LOCAL_CARGO_ENV = CARGO_INCREMENTAL=1
CRATES_IO_PUBLISH_ORDER = tmux-router \
	agent-doc-element \
	agent-doc-flow \
	agent-doc-hash \
	agent-doc-lease \
	agent-doc-log-time \
	agent-doc-prompt-cache \
	agent-doc-secret-redact \
	agent-doc-session-registry \
	agent-doc-tmux \
	agent-doc-tmux-commands \
	agent-doc-topic \
	agent-doc-debounce \
	agent-doc-element-boundary \
	agent-doc-element-icebox \
	agent-doc-element-queue \
	agent-doc-element-signals \
	agent-doc-element-status \
	agent-doc-element-unknown \
	agent-doc-fs \
	agent-doc-markdown-ast \
	agent-doc-prompt-cache-io \
	agent-doc-response-toc \
	agent-doc-session-accretion \
	agent-doc-syntax \
	agent-doc-ipc-protocol \
	agent-doc-lease-io \
	agent-doc-markdown-lossless \
	agent-doc-model-tier \
	agent-doc-tmux-io \
	agent-doc-callback-io \
	agent-doc-frontmatter \
	agent-doc-config \
	agent-doc-element-backlog \
	agent-doc-merge \
	agent-doc-project-config-io \
	agent-doc-turn-executor \
	agent-doc-write-command-io \
	agent-doc-element-done \
	agent-doc-element-review \
	agent-doc-frontmatter-io \
	agent-doc-memory \
	agent-doc-op-capture-io \
	agent-doc-prompt-lines \
	agent-doc-turn-executor-tmux \
	agent-doc-work-graph \
	agent-doc-diff \
	agent-doc-diff-io \
	agent-doc-element-exchange \
	agent-doc-prompt-context \
	agent-doc-prompt-contract \
	agent-doc-element-registry \
	agent-doc-harness \
	agent-doc-document \
	agent-doc-queue \
	agent-doc-template \
	agent-doc-ffi \
	agent-doc-supervisor \
	agent-doc-turn \
	agent-doc-controller \
	agent-doc-element-exchange-io \
	agent-doc-memory-io \
	agent-doc-owner-pane-io \
	agent-doc-sqlite \
	agent-doc-state-backbone \
	agent-doc-sync \
	agent-doc-turn-scope-io \
	agent-doc-workflow \
	agent-doc-document-realtime \
	agent-doc-process-owner-io \
	agent-doc-state-wire \
	agent-doc-git \
	agent-doc-plugin-owner \
	agent-doc-git-io \
	agent-doc-plugin-owner-io \
	agent-doc-agent-io \
	agent-doc-project-root-io \
	agent-doc-admin-io \
	agent-doc-archive-io \
	agent-doc-ipc-forensics-io \
	agent-doc-session-actor-io \
	agent-doc-turn-status-io \
	agent-doc-session-registry-io \
	agent-doc-focus-io \
	agent-doc-snapshot-io \
	agent-doc-supervisor-io \
	agent-doc-cycle-state-io \
	agent-doc-gc-io \
	agent-doc-merge-io \
	agent-doc-ops-log-io \
	agent-doc-run-context-io \
	agent-doc-workflow-io \
	agent-doc-capture-io \
	agent-doc-element-backlog-io \
	agent-doc-ipc-io \
	agent-doc-session-accretion-io \
	agent-doc-supervisor-process \
	agent-doc-codex-hook-io \
	agent-doc-crdt-relay-io \
	agent-doc-state-observer-io \
	agent-doc-supervisor-process-io \
	agent-doc-watch-io \
	agent-doc-compact-io \
	agent-doc-controller-io \
	agent-doc-queue-io \
	agent-doc-sync-io \
	agent-doc-test-support \
	agent-doc-write-converge-io \
	agent-doc-claim-io \
	agent-doc-preflight-io \
	agent-doc-flow-io \
	agent-doc-document-realtime-io \
	agent-doc-boundary-io \
	agent-doc-element-backlog-runtime-io \
	agent-doc-hooks-io \
	agent-doc-lint-io \
	agent-doc-prompt-io \
	agent-doc-response-replay-io \
	agent-doc-response-toc-io \
	agent-doc-session-check-io \
	agent-doc-status-io \
	agent-doc-template-io \
	agent-doc-prompt-context-io \
	agent-doc-repair-io \
	agent-doc-write-ipc-io \
	agent-doc-commit-io \
	agent-doc-run-io \
	agent-doc-stream-io \
	agent-doc-preflight-runtime-io \
	agent-doc-start-io \
	agent-doc-closeout-runtime-io \
	agent-doc-start-runtime-io \
	agent-doc-repair-runtime-io \
	agent-doc-route-io \
	agent-doc-run-runtime-io \
	agent-doc-write-runtime-io \
	agent-doc-codex-stop-io \
	agent-doc-preflight-command-io \
	agent-doc-repair-command-io \
	agent-doc
ifneq ($(strip $(LOCAL_RUSTFLAGS)),)
LOCAL_CARGO_ENV += RUSTFLAGS="$(LOCAL_RUSTFLAGS)"
endif

# Build debug binary
build:
	$(LOCAL_CARGO_ENV) cargo build

# Build release binary, cdylib, and symlink to .bin/
build-release:
	cargo build --release
	@mkdir -p .bin
	@ln -sf ../target/release/agent-doc .bin/agent-doc
	@agent-doc lib-install 2>/dev/null || true
	@echo "Installed .bin/agent-doc -> target/release/agent-doc"

# Release via CI: check, tag, push (CI builds + publishes), install locally
release: check
	@version=$$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/'); \
	echo "Releasing v$$version..."; \
	git tag "v$$version" && git push origin main "v$$version" && \
	echo "Tag v$$version pushed. CI handles GitHub Release + PyPI."; \
	$(MAKE) install-full

# Run tests (unset git hook env vars so temp-repo tests are not confused by GIT_DIR).
# Prefer cargo-nextest when installed; it runs test binaries concurrently while
# preserving Cargo's integration-test environment. Fall back to Cargo's own
# runner rather than reimplementing test execution.
test:
	@set -e; \
	if command -v cargo-nextest >/dev/null 2>&1; then \
		$(CARGO_CLEAN_ENV) cargo nextest run --workspace --all-targets $(NEXTEST_QUIET_FLAGS); \
		log=$$(mktemp "$${TMPDIR:-/tmp}/agent-doc-doctest.XXXXXX.log"); \
		if ! $(CARGO_CLEAN_ENV) cargo test --workspace --doc --quiet >"$$log" 2>&1; then \
			cat "$$log"; \
			rm -f "$$log"; \
			exit 1; \
		fi; \
		rm -f "$$log"; \
	else \
		log=$$(mktemp "$${TMPDIR:-/tmp}/agent-doc-test.XXXXXX.log"); \
		if ! $(CARGO_CLEAN_ENV) cargo test --workspace --all-targets --quiet -- --test-threads="$(TEST_THREADS)" >"$$log" 2>&1; then \
			cat "$$log"; \
			rm -f "$$log"; \
			exit 1; \
		fi; \
		rm -f "$$log"; \
	fi

# Wider deterministic simulator budget. Kept outside normal cargo test via
# #[ignore], but make check runs it explicitly so CI exercises more schedules.
sim-medium:
	@log=$$(mktemp "$${TMPDIR:-/tmp}/agent-doc-sim-medium.XXXXXX.log"); \
	if ! $(CARGO_CLEAN_ENV) cargo test closeout_sim_medium_seed_corpus_runs_wider_deterministic_budget --quiet -- --ignored --test-threads="$(TEST_THREADS)" >"$$log" 2>&1; then \
		cat "$$log"; \
		rm -f "$$log"; \
		exit 1; \
	fi; \
	rm -f "$$log"

# Live tmux integration sweep. These tests are intentionally ignored in the
# default development suite and run on CI where tmux is installed.
tmux-ci:
	$(CARGO_CLEAN_ENV) cargo test --all-targets -- --ignored --test-threads="$(TMUX_TEST_THREADS)"

# Lint
clippy:
	@cargo clippy --quiet -- -D warnings

# Verify Cargo.toml, pyproject.toml, and internal publish-unit versions match
version-sync:
	@cargo_ver=$$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/'); \
	pypi_ver=$$(grep '^version' pyproject.toml | head -1 | sed 's/.*"\(.*\)"/\1/'); \
	if [ "$$cargo_ver" != "$$pypi_ver" ]; then \
		echo "ERROR: version mismatch — Cargo.toml=$$cargo_ver pyproject.toml=$$pypi_ver"; \
		exit 1; \
	fi; \
	for manifest in */Cargo.toml; do \
		crate_ver=$$(grep '^version' "$$manifest" | head -1 | sed 's/.*"\(.*\)"/\1/'); \
		if [ "$$cargo_ver" != "$$crate_ver" ]; then \
			echo "ERROR: version mismatch — Cargo.toml=$$cargo_ver $$manifest=$$crate_ver"; \
			exit 1; \
		fi; \
	done

# Bump JB plugin patch version and build both zips
bump-plugin:
	@cd editors/jetbrains && \
	cur=$$(grep '^pluginVersion' gradle.properties | sed 's/.*= *//'); \
	maj=$$(echo "$$cur" | cut -d. -f1); \
	min=$$(echo "$$cur" | cut -d. -f2); \
	pat=$$(echo "$$cur" | cut -d. -f3); \
	new="$$maj.$$min.$$((pat + 1))"; \
	sed -i "s/^pluginVersion = .*/pluginVersion = $$new/" gradle.properties; \
	echo "bumped pluginVersion: $$cur -> $$new"; \
	./gradlew buildPlugin signPlugin && \
	ls -1 build/distributions/agent-doc-jetbrains-$$new*.zip

# Check plugin version was bumped if .kt files changed
plugin-version-check:
	@if git diff --cached --name-only 2>/dev/null | grep -q '\.kt$$'; then \
		if ! git diff --cached --name-only 2>/dev/null | grep -q 'gradle.properties'; then \
			echo "ERROR: .kt files changed but editors/jetbrains/gradle.properties pluginVersion not bumped"; \
			exit 1; \
		fi; \
	fi

# Build + machine-check the Lean formal models under formal/ (the wait-machinery
# `no_hang` bound proof, `#waitmachine4`). Skips gracefully when the Lean
# toolchain (lake) is not installed, so non-Lean environments / CI without elan
# are not blocked; when lake IS present the proofs must build clean.
lean:
	@if command -v lake >/dev/null 2>&1; then \
		for proj in formal/*/; do \
			if [ -f "$$proj/lakefile.toml" ] || [ -f "$$proj/lakefile.lean" ]; then \
				log=$$(mktemp "$${TMPDIR:-/tmp}/agent-doc-lean.XXXXXX.log"); \
				if ! ( cd "$$proj" && lake build ) >"$$log" 2>&1; then \
					echo "[lean] build failed: $$proj"; \
					cat "$$log"; \
					rm -f "$$log"; \
					exit 1; \
				fi; \
				rm -f "$$log"; \
			fi; \
		done; \
	else \
		echo "[lean] lake not found on PATH — skipping formal proof build (install elan/lean to verify formal/ proofs)"; \
	fi

# clippy + test + deterministic medium simulator corpus + version sync + Lean proofs
check: clippy test sim-medium version-sync lean

# Pre-commit: clippy + test + audit-docs + plugin version check
precommit: check plugin-version-check
	cargo run --quiet -- audit-docs

# Emit Cargo's build-timing report for local bottleneck analysis.
timings:
	$(LOCAL_CARGO_ENV) cargo build --timings

# Fast local install: reusable incremental target dir + local release profile.
install:
	$(LOCAL_CARGO_ENV) cargo install --path . --profile "$(LOCAL_INSTALL_PROFILE)" --target-dir "$(LOCAL_INSTALL_TARGET_DIR)" --force
	@$(LOCAL_CARGO_ENV) cargo build --profile "$(LOCAL_INSTALL_PROFILE)" --target-dir "$(LOCAL_INSTALL_TARGET_DIR)" --lib
	@CARGO_TARGET_DIR="$(LOCAL_INSTALL_TARGET_DIR)" agent-doc lib-install --profile "$(LOCAL_INSTALL_PROFILE)"

# Full optimized local install for pre-release parity.
install-full:
	cargo install --path . --force
	@cargo build --release --lib
	@agent-doc lib-install


# Remove the legacy full-suite pre-commit hook (works for both standalone repos and submodules)
install-hooks:
	@HOOK_DIR=$$(git rev-parse --git-dir)/hooks; \
	mkdir -p "$$HOOK_DIR"; \
	rm -f "$$HOOK_DIR/pre-commit"; \
	echo "Removed $$HOOK_DIR/pre-commit; run 'make check' explicitly after changes instead of relying on a git hook."

# Remove build artifacts
clean:
	cargo clean
	rm -f .bin/agent-doc

# Set up Python venv with maturin
init-python: PY_VERSION = $(shell [ -f .python-version ] && \
	cat .python-version || echo "3.14")
init-python:
	@echo "Setting up Python $(PY_VERSION) venv..."
	@if command -v mise >/dev/null 2>&1; then \
		mise install; \
	fi
	uv venv .venv --python "$(PY_VERSION)" --no-project --clear --seed $(VENV_ARGS)
	uv pip install maturin
	@echo "Venv ready. Use 'make wheel' to build, or '.venv/bin/maturin develop --release' to install into venv."

# Build wheel and install into venv for testing
wheel:
	.venv/bin/maturin develop --release

# Publish to crates.io
publish-crate:
	@set -e; \
	for crate in $(CRATES_IO_PUBLISH_ORDER); do \
		if [ "$$crate" = "tmux-router" ]; then \
			manifest="../tmux-router/Cargo.toml"; \
			publish_cmd="cargo publish --manifest-path ../tmux-router/Cargo.toml"; \
		else \
			manifest="Cargo.toml"; \
			publish_cmd="cargo publish -p $$crate"; \
		fi; \
		version=$$(awk 'found && /^version[[:space:]]*=/ { gsub(/"/, "", $$3); print $$3; exit } /^\[package\]/ { found=1 }' "$$manifest"); \
		if cargo search "$$crate" --limit 5 | grep -Eq "^$$crate = \"$$version\""; then \
			echo "[publish-crate] $$crate $$version already exists on crates.io; skipping"; \
			continue; \
		fi; \
		echo "[publish-crate] publishing $$crate $$version"; \
		$$publish_cmd; \
		echo "[publish-crate] waiting for $$crate $$version in crates.io index"; \
		for attempt in $$(seq 1 60); do \
			if cargo search "$$crate" --limit 5 | grep -Eq "^$$crate = \"$$version\""; then \
				echo "[publish-crate] $$crate $$version visible in crates.io index"; \
				break; \
			fi; \
			if [ "$$attempt" = "60" ]; then \
				echo "ERROR: $$crate $$version did not appear in crates.io index after publish"; \
				exit 1; \
			fi; \
			sleep 5; \
		done; \
	done

# Publish to PyPI
publish-pypi:
	.venv/bin/maturin publish --skip-existing

# Publish to both crates.io and PyPI
publish: publish-crate publish-pypi
