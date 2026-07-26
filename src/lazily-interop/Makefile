.PHONY: check fmt-check clippy test schema-check mutation-check

check: fmt-check clippy test schema-check mutation-check

fmt-check:
	cargo fmt --check

clippy:
	cargo clippy --all-targets -- -D warnings

test:
	cargo test --all

schema-check:
	cargo run --quiet -- schema-check

mutation-check:
	cargo run --quiet -- mutation-check
