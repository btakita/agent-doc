.PHONY: build build-release release test clippy check precommit install install-hooks clean init-python wheel publish publish-crate publish-pypi bump-plugin

# Build debug binary
build:
	cargo build

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
	cargo install --path .

# Run tests (unset git hook env vars so temp-repo tests are not confused by GIT_DIR)
test:
	env -u GIT_DIR -u GIT_INDEX_FILE -u GIT_WORK_TREE cargo test

# Lint
clippy:
	cargo clippy -- -D warnings

# Verify Cargo.toml and pyproject.toml versions match
version-sync:
	@cargo_ver=$$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/'); \
	pypi_ver=$$(grep '^version' pyproject.toml | head -1 | sed 's/.*"\(.*\)"/\1/'); \
	if [ "$$cargo_ver" != "$$pypi_ver" ]; then \
		echo "ERROR: version mismatch — Cargo.toml=$$cargo_ver pyproject.toml=$$pypi_ver"; \
		exit 1; \
	fi

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

# clippy + test + version sync
check: clippy test version-sync

# Pre-commit: clippy + test + audit-docs + plugin version check
precommit: check plugin-version-check
	cargo run --quiet -- audit-docs

# Install binary + versioned cdylib to ~/.cargo/bin
install:
	cargo install --path .
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
	cargo publish

# Publish to PyPI
publish-pypi:
	.venv/bin/maturin publish --skip-existing

# Publish to both crates.io and PyPI
publish: publish-crate publish-pypi
