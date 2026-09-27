.PHONY: release check precommit update publish clean check-shared publish-credential-proof review-gate-status review-gate-self-test

SUBMODULES = src/agent-doc src/corky

# Build release binaries for both projects
release:
	@for dir in $(SUBMODULES); do \
		echo "=== $$dir: make release ==="; \
		$(MAKE) -C $$dir release || exit 1; \
	done

# Run clippy + tests for both projects (includes shared crate)
check: check-shared publish-credential-proof review-gate-self-test
	@for dir in $(SUBMODULES); do \
		echo "=== $$dir: make check ==="; \
		$(MAKE) -C $$dir check || exit 1; \
	done

# Run clippy + tests for shared instruction-files crate
check-shared:
	@echo "=== instruction-files: clippy + test ==="
	@cd src/instruction-files && cargo clippy -- -D warnings && cargo test

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
	@cd src/instruction-files && cargo publish
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
	@cd src/instruction-files && cargo clean

# Fake-publisher and fresh-shell proof for the point-of-use publish credential
# plan (`#lzfakepublisherproof`, tasks/software/plan-ambient-publish-token-point-of-use.md).
# Resolves no credential and invokes no real publisher: `passage`, `cargo`,
# `maturin`, `bun`, and `python -m twine` are all replaced by recorders that
# return a fixed nonsecret sentinel. The self-test runs first because a proof
# whose assertions cannot fail is worse than no proof.
publish-credential-proof:
	@python3 scripts/proof-point-of-use-publish-credentials.py --self-test
	@python3 scripts/proof-point-of-use-publish-credentials.py

# Measure every gated `agent:review` item in tasks/software/lazily.md in one
# pass (`#advance-review`). Separates "still waiting on the operator" from
# "the gate's recorded evidence has rotted" -- the latter is actionable without
# the operator and is the only defect this pass has ever caught. Exits 1 when
# any gate's evidence is stale, 2 on refusal. Needs network (it shells out to
# the credential proof and the two macOS release probes), which is why only the
# offline self-test is a `check` dependency.
review-gate-status:
	@python3 scripts/review-gate-status.py --self-test
	@python3 scripts/review-gate-status.py

# Offline negative controls for the gate probes. Cheap, no network.
review-gate-self-test:
	@python3 scripts/review-gate-status.py --self-test
