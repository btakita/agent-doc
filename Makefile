.PHONY: build build-release release release-check release-preflight release-macos-assets release-macos-cadence-check release-version release-macos-coverage-check audit-docs test sim-medium sim-net sim-fuzz cross-editor-simworld editor-parity jetbrains-262-check tmux-ci clippy check check-fast dev-check-self-test release-driver-self-test python-compat-check artifact-purge-check precommit pypi-quota-check pypi-quota-self-test homebrew-formula-self-test timings install install-full install-editor-plugins editor-generation-bump cleanup-build-artifacts install-hooks clean init-python python-bootstrap-test wheel publish publish-pypi bump-plugin bump-plugin-262 version-sync dev-harness-test lean tla fuzz

CPU_COUNT ?= $(shell nproc 2>/dev/null || getconf _NPROCESSORS_ONLN 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo 4)
TEST_THREADS ?= 2
TMUX_TEST_THREADS ?= 1
CARGO_TARGET_DIR_ABS := $(abspath $(if $(strip $(CARGO_TARGET_DIR)),$(CARGO_TARGET_DIR),target))
# Build artifacts may live outside the repository, but install destinations follow Cargo's bin root.
CARGO_INSTALL_BIN_DIR_ABS := $(abspath $(if $(strip $(CARGO_INSTALL_ROOT)),$(CARGO_INSTALL_ROOT)/bin,$(if $(strip $(CARGO_HOME)),$(CARGO_HOME)/bin,$(if $(strip $(HOME)),$(HOME)/.cargo/bin,$(USERPROFILE)/.cargo/bin))))
AGENT_DOC_TEST_TMPDIR ?= $(if $(strip $(TMPDIR)),$(TMPDIR),$(shell if test -d /var/tmp && test -w /var/tmp; then printf '%s' /var/tmp; else printf '%s' /tmp; fi))
VSCODE_NODE_LOCK := editors/vscode/node_modules/.package-lock.json
CARGO_CLEAN_ENV = env -u GIT_DIR -u GIT_INDEX_FILE -u GIT_WORK_TREE
CARGO_CMD ?= ./scripts/with-cargo-cache cargo
NEXTEST_QUIET_FLAGS ?= --cargo-quiet --show-progress none --status-level fail --final-status-level fail --failure-output immediate-final --success-output never
BATCHED_TEST_PACKAGES := agent-doc-controller agent-doc-controller-io agent-doc-route-io agent-doc-session-check-io agent-doc-start-runtime-io
BATCHED_TEST_PACKAGE_ARGS := $(foreach package,$(BATCHED_TEST_PACKAGES),-p $(package))
NEXTEST_NON_BATCHED_FILTER := not (package(agent-doc-controller) | package(agent-doc-controller-io) | package(agent-doc-route-io) | package(agent-doc-session-check-io) | package(agent-doc-start-runtime-io))
LOCAL_INSTALL_PROFILE ?= release-local
LOCAL_INSTALL_TARGET_DIR ?= target/local-install
LOCAL_LINKER ?= $(shell if command -v mold >/dev/null 2>&1; then printf '%s' mold; elif command -v ld.lld >/dev/null 2>&1 || command -v lld >/dev/null 2>&1; then printf '%s' lld; fi)
LOCAL_RUSTFLAGS ?= $(if $(LOCAL_LINKER),-C link-arg=-fuse-ld=$(LOCAL_LINKER),)
LOCAL_CARGO_ENV = CARGO_INCREMENTAL=1
ifneq ($(strip $(LOCAL_RUSTFLAGS)),)
LOCAL_CARGO_ENV += RUSTFLAGS="$(LOCAL_RUSTFLAGS)"
endif

# Build debug binary
build:
	$(LOCAL_CARGO_ENV) $(CARGO_CMD) build

# Build release binary, cdylib, and symlink to .bin/
build-release:
	$(CARGO_CMD) build --release
	@mkdir -p .bin
	@ln -sf ../target/release/agent-doc .bin/agent-doc
	@agent-doc lib-install 2>/dev/null || true
	@echo "Installed .bin/agent-doc -> target/release/agent-doc"

# Release via CI: check, tag, push, then install locally. GitHub Actions builds
# Linux, Windows, and both Darwin archives for every tag.
# The release owns the one authoritative full-suite gate. A content-identical
# successful `make check` and `make tmux-ci` performed after integration/version
# projection are reused independently; source/toolchain changes invalidate both.
release: release-check
	@version=$$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/'); \
	python3 scripts/release-driver.py --version "$$version" --make "$(MAKE)"

release-preflight: version-sync
	@python3 scripts/check_editor_parity.py

release-check: release-preflight
	@if ! python3 scripts/dev-check.py verify-full-check; then \
		$(MAKE) check; \
	fi
	@python3 scripts/dev-check.py verify-full-check
	@if ! python3 scripts/dev-check.py verify-tmux-ci; then \
		$(MAKE) tmux-ci; \
	fi
	@python3 scripts/dev-check.py verify-tmux-ci

release-macos-cadence-check:
	@python3 scripts/agent-doc-dev verify-macos-release-cadence

# Report releases published after the last Darwin upload that dropped either
# Darwin archive. Deliberately NOT part of `check`: clearing it needs Mac
# hardware, so wiring it into the build would redden every unrelated change.
release-macos-coverage-check:
	@python3 scripts/agent-doc-dev verify-macos-release-coverage

# Repair both Darwin archives from a Mac and attach them to an existing release.
# Usage: make release-macos-assets TAG=v0.35.398
release-macos-assets:
	@test -n "$(TAG)" || (echo "ERROR: TAG is required (for example, make release-macos-assets TAG=v0.35.398)" && exit 1)
	@scripts/release-macos-assets "$(TAG)"

# Project one release version across packages, internal dependency constraints,
# lockfile entries, Python metadata, and both shipped/development skill copies.
release-version:
	@test -n "$(VERSION)" || (echo "ERROR: VERSION is required (for example, make release-version VERSION=0.35.89)" && exit 1)
	@python3 scripts/agent-doc-dev release-version "$(VERSION)"
	@# `#skillinstallstalemirror`: installed copies are the installer's output,
	@# not a sed target. `--root .` reaches the submodule-local install that bare
	@# root resolution skips in favour of the superproject.
	@$(LOCAL_CARGO_ENV) $(CARGO_CMD) run --quiet --bin agent-doc -- skill install --root . --all
	@$(LOCAL_CARGO_ENV) $(CARGO_CMD) run --quiet --bin agent-doc -- skill install --all

# Run tests (unset git hook env vars so temp-repo tests are not confused by GIT_DIR).
# Prefer cargo-nextest when installed; it runs ordinary test binaries
# concurrently. Controller-heavy packages use one cargo-test process per test
# binary instead: nextest's one-process-per-test model repeatedly paid controller
# initialization cost. The fallback remains one workspace-wide cargo-test run.
test sim-medium sim-net cross-editor-simworld dev-harness-test editor-parity tmux-ci check: export TMPDIR := $(AGENT_DOC_TEST_TMPDIR)

$(VSCODE_NODE_LOCK): editors/vscode/package.json editors/vscode/package-lock.json
	npm ci --prefix editors/vscode

test:
	@set -e; \
	test_agent_doc_bin="$(CARGO_TARGET_DIR_ABS)/debug/agent-doc"; \
	$(CARGO_CLEAN_ENV) $(CARGO_CMD) build --bin agent-doc --lib --quiet; \
	if command -v cargo-nextest >/dev/null 2>&1; then \
		if ! AGENT_DOC_BIN="$$test_agent_doc_bin" $(CARGO_CLEAN_ENV) $(CARGO_CMD) nextest run --workspace --all-targets -E '$(NEXTEST_NON_BATCHED_FILTER)' $(NEXTEST_QUIET_FLAGS); then \
			exit 1; \
		fi; \
		log=$$(mktemp "$${TMPDIR:-/tmp}/agent-doc-batched-test.XXXXXX.log"); \
		if ! AGENT_DOC_BIN="$$test_agent_doc_bin" $(CARGO_CLEAN_ENV) $(CARGO_CMD) test $(BATCHED_TEST_PACKAGE_ARGS) --all-targets --quiet -- --test-threads="$(TEST_THREADS)" >"$$log" 2>&1; then \
			cat "$$log"; \
			rm -f "$$log"; \
			exit 1; \
		fi; \
		rm -f "$$log"; \
		log=$$(mktemp "$${TMPDIR:-/tmp}/agent-doc-doctest.XXXXXX.log"); \
		if ! AGENT_DOC_BIN="$$test_agent_doc_bin" $(CARGO_CLEAN_ENV) $(CARGO_CMD) test --workspace --doc --quiet >"$$log" 2>&1; then \
			cat "$$log"; \
			rm -f "$$log"; \
			exit 1; \
		fi; \
		rm -f "$$log"; \
	else \
		log=$$(mktemp "$${TMPDIR:-/tmp}/agent-doc-test.XXXXXX.log"); \
		if ! AGENT_DOC_BIN="$$test_agent_doc_bin" $(CARGO_CLEAN_ENV) $(CARGO_CMD) test --workspace --all-targets --quiet -- --test-threads="$(TEST_THREADS)" >"$$log" 2>&1; then \
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
	if ! $(CARGO_CLEAN_ENV) $(CARGO_CMD) test closeout_sim_medium_seed_corpus_runs_wider_deterministic_budget --quiet -- --ignored --test-threads="$(TEST_THREADS)" >"$$log" 2>&1; then \
		cat "$$log"; \
		rm -f "$$log"; \
		exit 1; \
	fi; \
	rm -f "$$log"

# `#netadv4`: the fast corpus schedules (seeds 0..512) under the seeded
# `coder_zscaler` and `hostile` network profiles, each across the fixed net seeds
# in `src/sim_world/net.rs` (NET_CORPUS_SEEDS). Structural invariants and the
# corpus coverage floor must hold; any oracle finding class outside
# KNOWN_OPEN_NET_FINDINGS fails the run. Scripted scenarios can be replayed under a
# profile ad hoc: AGENT_DOC_SIM_NET_PROFILE=hostile AGENT_DOC_SIM_NET_SEED=3 cargo test sim_world::
sim-net:
	@log=$$(mktemp "$${TMPDIR:-/tmp}/agent-doc-sim-net.XXXXXX.log"); \
	if ! $(CARGO_CLEAN_ENV) $(CARGO_CMD) test --bin agent-doc sim_world::net::tests::closeout_sim_net_ --quiet -- --ignored --test-threads="$(TEST_THREADS)" >"$$log" 2>&1; then \
		cat "$$log"; \
		rm -f "$$log"; \
		exit 1; \
	fi; \
	rm -f "$$log"

# `#netadv6`: deterministic simulation fuzzing on FRESH seeds for FUZZ_SECS
# seconds (src/sim_world/fuzz.rs). `make check` already runs the short fixed
# budget and replays every regression seed in src/sim_world/fuzz_seeds.txt
# through `make test`. Each NEW finding kind's shrunk, replayable trace is printed
# and written to FUZZ_OUT/<kind>.trace; paste it into fuzz_seeds.txt to make it a
# permanent regression seed. FUZZ_SEED pins the base seed (default: clock).
FUZZ_SECS ?= 600
FUZZ_STEPS ?= 160
FUZZ_OUT ?= $(CARGO_TARGET_DIR_ABS)/sim-fuzz
sim-fuzz:
	@mkdir -p "$(FUZZ_OUT)"; log="$(FUZZ_OUT)/sim-fuzz.log"; \
	if AGENT_DOC_SIM_FUZZ_SECS="$(FUZZ_SECS)" AGENT_DOC_SIM_FUZZ_STEPS="$(FUZZ_STEPS)" \
		AGENT_DOC_SIM_FUZZ_OUT="$(FUZZ_OUT)" $(if $(FUZZ_SEED),AGENT_DOC_SIM_FUZZ_SEED="$(FUZZ_SEED)") \
		$(CARGO_CLEAN_ENV) $(CARGO_CMD) test --bin agent-doc sim_world::fuzz::tests::sim_fuzz_long_budget \
		-- --ignored --nocapture --test-threads=1 >"$$log" 2>&1; then status=0; else status=1; fi; \
	grep -v '^\[template\]' "$$log" | grep -v '^\[perf\]'; \
	echo "sim-fuzz: exit=$$status log=$$log out=$(FUZZ_OUT)"; \
	exit $$status

# Compile and run the shipped JetBrains and VS Code CRDT forwarders, controller
# transports, and native FFI nodes as peers through a real agent-doc controller.
# Zed stays staged by editors/plugin-parity.tsv until its native endpoint exists.
cross-editor-simworld: $(VSCODE_NODE_LOCK)
	@set -e; \
	test_agent_doc_bin="$(CARGO_TARGET_DIR_ABS)/debug/agent-doc"; \
	$(CARGO_CLEAN_ENV) $(CARGO_CMD) build --bin agent-doc --lib --quiet; \
	( cd editors/vscode && npm run compile ); \
	( cd editors/jetbrains && ./gradlew --no-daemon --console=plain -q testClasses ); \
	AGENT_DOC_BIN="$$test_agent_doc_bin" $(CARGO_CLEAN_ENV) $(CARGO_CMD) test --test cross_editor_simworld native_plugin_harnesses_peer_through_real_agent_doc_controller -- --ignored --nocapture --test-threads=1

# Live tmux integration sweep. These tests are intentionally ignored in the
# default development suite and run on CI where tmux is installed.
tmux-ci:
	@set -e; \
	test_agent_doc_bin="$(CARGO_TARGET_DIR_ABS)/debug/agent-doc"; \
	$(CARGO_CLEAN_ENV) $(CARGO_CMD) build --bin agent-doc --quiet; \
	AGENT_DOC_BIN="$$test_agent_doc_bin" $(CARGO_CLEAN_ENV) $(CARGO_CMD) test --all-targets -- --ignored --skip native_plugin_harnesses_peer_through_real_agent_doc_controller --test-threads="$(TMUX_TEST_THREADS)"; \
	AGENT_DOC_BIN="$$test_agent_doc_bin" $(CARGO_CLEAN_ENV) $(CARGO_CMD) test -p agent-doc-sync-io repair_layout_ -- --ignored --test-threads="$(TMUX_TEST_THREADS)"; \
	AGENT_DOC_BIN="$$test_agent_doc_bin" $(CARGO_CLEAN_ENV) $(CARGO_CMD) test -p agent-doc-route-io --lib layout_startup_completion_cannot_append_a_third_visible_pane -- --ignored --test-threads="$(TMUX_TEST_THREADS)"; \
	for test_filter in provision_pane_ manual_layout_provisions_paused_queue_without_dispatch_or_resume layout_owned_provisioning_does_not_focus_intermediate_pane; do \
		AGENT_DOC_BIN="$$test_agent_doc_bin" $(CARGO_CLEAN_ENV) $(CARGO_CMD) test -p agent-doc-route-io --test route "$$test_filter" -- --ignored --test-threads="$(TMUX_TEST_THREADS)"; \
	done
	@AGENT_DOC_TMUX_CI_SUCCEEDED=1 python3 scripts/dev-check.py record-tmux-ci

# Lint
clippy:
	@$(CARGO_CMD) clippy --quiet --all-targets --all-features -- -D warnings

# Verify the complete release projection and every agent-doc crate's privacy.
version-sync:
	@python3 scripts/agent-doc-dev verify-release-version

dev-harness-test: $(VSCODE_NODE_LOCK)
	@python3 scripts/agent-doc-dev self-test
	@cd editors/jetbrains && ./gradlew --no-daemon --console=plain -q test
	@cd editors/vscode && npm test

# Every release records coverage, tests both supported plugins, and executes
# their shipped native forwarders together through a real controller.
editor-parity: dev-harness-test cross-editor-simworld
	@python3 scripts/check_editor_parity.py

# The 262 modular ZIP is a distinct compatibility-ranged artifact. Keep its
# frontend/backend/both descriptor contract and BOTH sandboxes in the same
# authoritative gate as the classic editor packages.
jetbrains-262-check:
	@cd editors/jetbrains-262 && gradle --no-daemon --console=plain test buildPlugin verifySplitArtifact verifySplitModeSandboxes

# Bump JB plugin patch version (when its sources changed) and build both zips.
# `#installgenskew`: the bump goes through check_plugin_versions.py so it also
# records `pluginSourceDigest`. A bare `sed` bump left the digest stale, and the
# next install then bumped AGAIN after the binary had been built, shipping plugin
# 0.2.443 beside a binary that expected 0.2.442 (plugin_generation_mismatch).
bump-plugin:
	@python3 scripts/check_plugin_versions.py --bump JetBrains
	@cd editors/jetbrains && \
	new=$$(grep '^pluginVersion' gradle.properties | sed 's/.*= *//'); \
	./gradlew buildPlugin signPlugin && \
	ls -1 build/distributions/agent-doc-jetbrains-$$new*.zip

bump-plugin-262:
	@python3 scripts/check_plugin_versions.py --bump "JetBrains 262"
	@cd editors/jetbrains-262 && \
	new=$$(grep '^pluginVersion' gradle.properties | sed 's/.*= *//'); \
	gradle --no-daemon --console=plain buildPlugin verifySplitArtifact && \
	ls -1 build/distributions/agent-doc-jetbrains-262-$$new.zip

# `#installgenskew`: the binary embeds the JetBrains generation it expects
# (agent-doc-reliable-sync-io/build.rs reads gradle.properties), so any bump
# must land BEFORE the binary builds. install-editor-plugins bumps again only if
# sources changed in between, which this makes a no-op.
editor-generation-bump:
	@if agent-doc plugin list 2>/dev/null | grep -q '^jetbrains'; then \
		python3 scripts/check_plugin_versions.py --bump JetBrains || exit 1; \
	fi

# Check staged changes and committed history since each target's last package
# generation. This catches a same-version plugin behavior commit even when the
# normal check runs after that commit, while ignoring unrelated unstaged work.
plugin-version-check:
	@python3 scripts/check_plugin_versions.py --self-test
	@python3 scripts/check_plugin_versions.py

# Every tracked `*.py` must parse under pyproject's `requires-python` floor (gh #117).
# A 3.12-only construct (PEP 701 f-string expressions) lands green on a 3.12+
# runner and breaks `make check` for every pre-3.12 developer, so this runs first
# and refuses those constructs on any interpreter. Offline, no network.
python-compat-check:
	@python3 scripts/check_python_compat.py --self-test
	@python3 scripts/check_python_compat.py

# Refusal-path regressions for the artifact purge executor (`#lzartifactpurgeexec`).
# Offline only: the self-test never contacts the Actions API and never deletes.
# The live dry run needs `gh` auth, so it stays an explicit operator invocation
# (`python3 scripts/purge-actions-artifacts.py`).
artifact-purge-check:
	@python3 scripts/purge-actions-artifacts.py --self-test

# PyPI storage headroom + limit-request status, unauthenticated (`#pypislim`).
# Reads per-file sizes from the PEP 691 simple index, NOT `pypi.org/pypi/<name>/json`
# (which has served a stale CDN view listing deleted releases) and NOT the
# authenticated `/manage/project/` settings page (PyPI gates it behind a password
# re-confirmation, which is what used to stall this check on a human). Exits 1 only
# when a ceiling is actually in reach; an unreachable ceiling is never a reason to
# delete release history.
pypi-quota-check:
	@python3 scripts/pypi-quota-check.py

# Offline arithmetic + verdict thresholds for the check above. Part of `make check`;
# the live network read stays an explicit invocation (`make pypi-quota-check`).
pypi-quota-self-test:
	@python3 scripts/pypi-quota-check.py --self-test

# Fixture regressions for the Homebrew formula generator (GH #31). Offline: the
# Homebrew workflow feeds it the release's real SHA256SUMS on every tag.
homebrew-formula-self-test:
	@python3 scripts/bump-homebrew-formula.py --self-test

# Build + machine-check the Lean formal models under formal/ (including the
# wait-machine bound and captured-response closeout safety/completeness proofs).
# Skips gracefully when the Lean
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

# clippy + tests + deterministic simulator corpus + release harness + formal checks
#
# `audit-docs` belongs here, not only in `precommit` (#skillinstallstalemirror):
# the release process runs `make check`, so leaving the installed-surface audit
# out of it let 0.35.224 ship with harness runbooks several versions behind the
# binary while every version marker matched.
dev-check-self-test:
	@python3 scripts/dev-check.py self-test

release-driver-self-test:
	@python3 scripts/release-driver.py --self-test

# Fast edit-loop validation: helper checks plus Rust packages affected by the
# diff and their reverse-dependency closure. This is not a release proof.
check-fast: python-compat-check plugin-version-check artifact-purge-check pypi-quota-self-test homebrew-formula-self-test dev-check-self-test release-driver-self-test
	@python3 scripts/dev-check.py run

check: python-compat-check plugin-version-check artifact-purge-check pypi-quota-self-test homebrew-formula-self-test dev-check-self-test release-driver-self-test clippy test sim-medium sim-net version-sync audit-docs editor-parity jetbrains-262-check python-bootstrap-test lean tla
	@AGENT_DOC_FULL_CHECK_SUCCEEDED=1 python3 scripts/dev-check.py record-full-check

# Audit generated instruction surfaces (skill, runbooks, OKF) against the binary.
audit-docs:
	@$(CARGO_CMD) run --quiet -- audit-docs

# Translate the PlusCal concurrency model and check its TLA+ safety/liveness properties.
tla:
	@./scripts/run_tla.sh

# Coverage-guided fuzzing of untrusted input (`#netadv7`). Opt-in: needs a
# nightly toolchain and `cargo install cargo-fuzz`, so it is NOT part of
# `check`. `check` still exercises every target on stable: the
# `agent-doc-fuzz-harness` tests replay fuzz/corpus and fuzz/regressions
# through the same harness functions. New inputs go to a scratch corpus so the
# committed seed corpus stays small; promote a crasher by minimizing it
# (`cargo +nightly fuzz tmin`) into fuzz/regressions/<target>/ and adding a
# unit regression test beside the fix.
FUZZ_SECONDS ?= 60
FUZZ_TARGETS ?= ipc_wire markdown_patch frontmatter crdt_update crdt_edits
FUZZ_WORK ?= $(or $(TMPDIR),/tmp)/agent-doc-fuzz
fuzz:
	@set -e; \
	for target in $(FUZZ_TARGETS); do \
		mkdir -p "$(FUZZ_WORK)/$$target"; \
		echo "fuzz: $$target for $(FUZZ_SECONDS)s"; \
		$(CARGO_CMD) +nightly fuzz run -O $$target "$(FUZZ_WORK)/$$target" fuzz/corpus/$$target fuzz/regressions/$$target -- \
			-max_total_time=$(FUZZ_SECONDS) -timeout=10 -rss_limit_mb=2048 -max_len=16384; \
	done

# Pre-commit: clippy + test + audit-docs + plugin version check
precommit: check

# Emit Cargo's build-timing report for local bottleneck analysis.
timings:
	$(LOCAL_CARGO_ENV) $(CARGO_CMD) build --timings

# Fast local install: reusable incremental target dir + local release profile.
# Build first, then let the freshly-built binary atomically replace the installed
# executable. `cargo install --force` unlinks the old executable before persisting
# the new one, creating a short ENOENT window that can strand controller/supervisor
# execve handoffs.
# `#installbuildskew`: the binary and the cdylib are built by ONE cargo
# invocation, before either is installed. Two invocations each re-run build.rs,
# whose IPC build id is a digest of the working tree; a concurrent session
# editing the shared tree between them gave the binary 3d7dff.. and the cdylib
# c7a28f.. under one version (2026-09-29), and every editor handshake was then
# refused until the next install. `lib-install` also refuses that skew.
install: editor-generation-bump
	$(LOCAL_CARGO_ENV) $(CARGO_CMD) build --profile "$(LOCAL_INSTALL_PROFILE)" --target-dir "$(LOCAL_INSTALL_TARGET_DIR)" --bin agent-doc --lib
	@"$(LOCAL_INSTALL_TARGET_DIR)/$(LOCAL_INSTALL_PROFILE)/agent-doc" binary-install --source "$(LOCAL_INSTALL_TARGET_DIR)/$(LOCAL_INSTALL_PROFILE)/agent-doc"
	@"$(LOCAL_INSTALL_TARGET_DIR)/$(LOCAL_INSTALL_PROFILE)/agent-doc" skill install --all
	@"$(LOCAL_INSTALL_TARGET_DIR)/$(LOCAL_INSTALL_PROFILE)/agent-doc" skill install --root . --all
	@CARGO_TARGET_DIR="$(LOCAL_INSTALL_TARGET_DIR)" agent-doc lib-install --profile "$(LOCAL_INSTALL_PROFILE)"
	@$(MAKE) install-editor-plugins

# Pre-release parity install. NOT the dev-loop install -- use `make install`.
#
# `#installfulloom`: this builds `[profile.release]`, which is `lto = "fat"` +
# `codegen-units = 1` across a 144-crate workspace. Fat LTO collapses that into
# one enormous LLVM process, so peak memory is far higher than a normal build and
# repeated runs can OOM the machine (observed 2026-07-18: a session that called
# this ~10 times during iteration was killed with SIGKILL/137).
#
# `make install` builds `release-local` (lto = off, codegen-units = 256,
# incremental) and is the correct install for the edit -> install -> recycle
# loop. Reserve `install-full` for verifying pre-release parity, which is what
# the `release` target uses it for.
install-full: editor-generation-bump
	$(CARGO_CMD) build --release --target-dir "$(CARGO_TARGET_DIR_ABS)" --bin agent-doc --lib
	@"$(CARGO_TARGET_DIR_ABS)/release/agent-doc" binary-install --source "$(CARGO_TARGET_DIR_ABS)/release/agent-doc"
	@"$(CARGO_TARGET_DIR_ABS)/release/agent-doc" skill install --all
	@"$(CARGO_TARGET_DIR_ABS)/release/agent-doc" skill install --root . --all
	@CARGO_TARGET_DIR="$(CARGO_TARGET_DIR_ABS)" "$(CARGO_TARGET_DIR_ABS)/release/agent-doc" lib-install --profile release --target-dir "$(CARGO_INSTALL_BIN_DIR_ABS)"
	@$(MAKE) install-editor-plugins
	@$(MAKE) cleanup-build-artifacts

# Keep every existing JetBrains and VS Code agent-doc package on the source generation.
# `#jbversionbumpperbuild`: bump the JetBrains generation FIRST when its sources
# differ from the last packaged one, so a rebuild cannot ship distinct bytes under
# a version string that already shipped. Two 0.2.388 builds on 2026-09-20 did, and
# the running IDE then mapped an unlinked jar whose version claimed to be current.
# The bump is conditional, not per-invocation: an unchanged rebuild holds its
# version, so this adds no churn to a no-op `make install`.
# The native cdylib and editor package are separate install surfaces: updating
# only the former leaves running turns reporting the older package generation.
install-editor-plugins:
	@if agent-doc plugin list 2>/dev/null | grep -q '^jetbrains'; then \
		python3 scripts/check_plugin_versions.py --bump JetBrains || exit 1; \
		( cd editors/jetbrains && ./gradlew buildPlugin ) || { \
			echo "JetBrains plugin build failed. Use a JDK 21-compatible Gradle runtime (set JAVA_HOME to JDK 21). Refusing to install a stale package." >&2; \
			exit 1; \
		}; \
		agent-doc plugin install jetbrains --local --all-installed; \
	else \
		echo "No existing JetBrains agent-doc package; editor package sync skipped."; \
	fi
	@if agent-doc plugin list 2>/dev/null | grep -q '^vscode'; then \
		if [ -x editors/vscode/node_modules/.bin/vsce ] || command -v vsce >/dev/null 2>&1; then \
			( cd editors/vscode && npm run package ) || exit 1; \
			agent-doc plugin install vscode --local; \
		else \
			echo "WARNING: vsce is not installed; VS Code package sync skipped. The installed VS Code agent-doc package stays on its previous generation. Run \`npm install\` in editors/vscode (or install @vscode/vsce globally), then re-run \`make install-editor-plugins\` to refresh it." >&2; \
		fi; \
	else \
		echo "No existing VS Code agent-doc package; editor package sync skipped."; \
	fi

# A full release install leaves the executable, native library, and plugins in
# their installed destinations, so repo-owned compilation outputs are no longer
# required. Preserve Cargo's global dependency caches; opt out when another
# incremental build is imminent with AGENT_DOC_CLEAN_BUILD_ARTIFACTS=0.
cleanup-build-artifacts:
	@scripts/cleanup-build-artifacts.sh


# Remove the legacy full-suite pre-commit hook (works for both standalone repos and submodules)
install-hooks:
	@HOOK_DIR=$$(git rev-parse --git-dir)/hooks; \
	mkdir -p "$$HOOK_DIR"; \
	rm -f "$$HOOK_DIR/pre-commit"; \
	echo "Removed $$HOOK_DIR/pre-commit; run 'make check' explicitly after changes instead of relying on a git hook."

# Remove build artifacts
clean:
	$(CARGO_CMD) clean
	rm -f .bin/agent-doc

# Set up a Python venv for building and publishing the bootstrap wheel.
init-python: PY_VERSION = $(shell [ -f .python-version ] && \
	cat .python-version || echo "3.14")
init-python:
	@echo "Setting up Python $(PY_VERSION) venv..."
	@if command -v mise >/dev/null 2>&1; then \
		mise install; \
	fi
	uv venv .venv --python "$(PY_VERSION)" --no-project --clear --seed $(VENV_ARGS)
	uv pip install build twine
	@echo "Venv ready. Use 'make wheel' to build the universal bootstrap wheel."

python-bootstrap-test:
	@python3 -m unittest discover -s python/tests -v

# Build the native-free universal bootstrap wheel.
wheel: python-bootstrap-test
	rm -rf dist
	.venv/bin/python -m build --wheel

# Publish to PyPI
publish-pypi:
	.venv/bin/python -m twine upload --skip-existing dist/*

# agent-doc's Rust workspace is private; release binaries through GitHub and PyPI.
publish: publish-pypi
