.PHONY: release check precommit update publish clean check-shared

SUBMODULES = agent-doc corky

# Build release binaries for both projects
release:
	@for dir in $(SUBMODULES); do \
		echo "=== $$dir: make release ==="; \
		$(MAKE) -C $$dir release || exit 1; \
	done

# Run clippy + tests for both projects (includes shared crate)
check: check-shared
	@for dir in $(SUBMODULES); do \
		echo "=== $$dir: make check ==="; \
		$(MAKE) -C $$dir check || exit 1; \
	done

# Run clippy + tests for shared instruction-files crate
check-shared:
	@echo "=== instruction-files: clippy + test ==="
	@cd instruction-files && cargo clippy -- -D warnings && cargo test

# Pre-commit checks for both projects + workspace-level audit
precommit: check-shared
	@for dir in $(SUBMODULES); do \
		echo "=== $$dir: make precommit ==="; \
		$(MAKE) -C $$dir precommit || exit 1; \
	done
	@echo "=== workspace: library context policy ==="
	@cd src/instruction-files && cargo run --quiet -- check-policy --root ../..

# Pull latest changes and rebuild release binaries
update:
	@for dir in $(SUBMODULES); do \
		echo "=== $$dir: git pull + make release ==="; \
		git -C $$dir pull --ff-only || exit 1; \
		$(MAKE) -C $$dir release || exit 1; \
	done
	@echo "Updated submodules to latest. Run 'git add agent-doc corky && git commit' to pin refs."

# Publish: shared crate first, then both projects
publish:
	@echo "=== instruction-files: cargo publish ==="
	@cd instruction-files && cargo publish
	@for dir in $(SUBMODULES); do \
		echo "=== $$dir: make publish ==="; \
		$(MAKE) -C $$dir publish || exit 1; \
	done

# Clean build artifacts for both projects
clean:
	@for dir in $(SUBMODULES); do \
		echo "=== $$dir: make clean ==="; \
		$(MAKE) -C $$dir clean; \
	done
	@cd instruction-files && cargo clean
