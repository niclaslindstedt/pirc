.PHONY: all build test lint fmt fmt-check clean check

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

clean:
	cargo clean

all: fmt-check lint build test

check: all
