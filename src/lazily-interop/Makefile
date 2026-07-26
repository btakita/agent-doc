.PHONY: check fmt-check clippy test schema-check mutation-check prepare-network-peers \
	network-suite network-suite-pr network-suite-merge network-suite-nightly

NETWORK_SEED ?= 17
NETWORK_TIER ?= local
REPORT_DIR ?= reports

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

prepare-network-peers:
	cmake -S ../lazily-cpp -B ../lazily-cpp/build
	cmake --build ../lazily-cpp/build --target lazily_interop_peer --parallel

network-suite: prepare-network-peers
	mkdir -p $(REPORT_DIR)
	cargo run --quiet -- run --scenario scenarios/eight-way-convergence.json \
		--seed $(NETWORK_SEED) --timeout-ms 10000 --require-channels \
		--format json --output $(REPORT_DIR)/$(NETWORK_TIER)-semantic.json
	cargo run --quiet -- run --scenario scenarios/channel-matrix.json \
		--seed $(NETWORK_SEED) --timeout-ms 10000 --require-channels \
		--format json --output $(REPORT_DIR)/$(NETWORK_TIER)-channels.json
	@echo "lazily-interop: $(NETWORK_TIER) network suite OK (seed $(NETWORK_SEED))"

network-suite-pr:
	$(MAKE) network-suite NETWORK_TIER=pr NETWORK_SEED=17

network-suite-merge:
	$(MAKE) network-suite NETWORK_TIER=merge NETWORK_SEED=29

network-suite-nightly:
	$(MAKE) network-suite NETWORK_TIER=nightly-17 NETWORK_SEED=17
	$(MAKE) network-suite NETWORK_TIER=nightly-29 NETWORK_SEED=29
	$(MAKE) network-suite NETWORK_TIER=nightly-43 NETWORK_SEED=43
