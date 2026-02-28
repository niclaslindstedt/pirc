.PHONY: all build test lint fmt fmt-check clean check bench perf-test doc

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

all: fmt-check lint build test

check: all
