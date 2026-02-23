.PHONY: build release test clippy check precommit install install-hooks clean init-python wheel publish publish-crate publish-pypi

# Build debug binary
build:
	cargo build

# Build release binary and symlink to .bin/
release:
	cargo build --release
	@mkdir -p .bin
	@ln -sf ../target/release/agent-doc .bin/agent-doc
	@echo "Installed .bin/agent-doc -> target/release/agent-doc"

# Run tests
test:
	cargo test

# Lint
clippy:
	cargo clippy -- -D warnings

# clippy + test
check: clippy test

# Pre-commit: clippy + test + audit-docs
precommit: check
	cargo run --quiet -- audit-docs

# Install to ~/.cargo/bin
install:
	cargo install --path .

# Install git hooks
install-hooks:
	@mkdir -p .git/hooks
	@printf '#!/bin/sh\nmake precommit\n' > .git/hooks/pre-commit
	@chmod +x .git/hooks/pre-commit
	@echo "Installed .git/hooks/pre-commit"

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
