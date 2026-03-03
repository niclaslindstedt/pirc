.PHONY: all build test lint fmt fmt-check clean check bench perf-test doc release install dev dev-down dev-logs

INSTALL_DIR ?= $(shell if [ -w /usr/local/bin ]; then echo /usr/local/bin; elif [ -d $(HOME)/.local/bin ]; then echo $(HOME)/.local/bin; else echo $(HOME)/.local/bin; fi)

build:
	cargo build --workspace

test:
	RUST_MIN_STACK=16777216 cargo test --workspace

lint:
	cargo clippy --workspace -- -D warnings

fmt:
	cargo fmt --all

fmt-check:
	cargo fmt --all -- --check

bench:
	cargo bench -p pirc-protocol -p pirc-server -p pirc-crypto -p pirc-network

perf-test:
	RUST_MIN_STACK=16777216 cargo test -p pirc-integration-tests --test perf_validation -- --ignored --nocapture

doc:
	cargo doc --workspace --no-deps

clean:
	cargo clean

install: release-build
	@mkdir -p $(INSTALL_DIR)
	install -m 755 target/release/pirc $(INSTALL_DIR)/pirc
	install -m 755 target/release/pircd $(INSTALL_DIR)/pircd
	@echo "Installed pirc and pircd to $(INSTALL_DIR)"

release-build:
	cargo build --release --workspace

release:
	@bash scripts/release.sh $(BUMP)

all: fmt-check lint build test

check: all

dev:
	docker build -f docker/pircd/Dockerfile.dev -t pirc-dev:latest .
	docker compose -f docker-compose.dev.yml up -d
	@echo "Waiting for dev cluster to bootstrap..."
	@sleep 15
	cargo run --bin pirc -- --config docker/dev-pirc.toml

dev-down:
	docker compose -f docker-compose.dev.yml down -v

dev-logs:
	docker compose -f docker-compose.dev.yml logs -f
