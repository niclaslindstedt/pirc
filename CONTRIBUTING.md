# Contributing to pirc

## Development Setup

### Prerequisites

- **Rust** stable toolchain (MSRV: 1.85)
- **Make** for build commands

Install Rust via [rustup](https://rustup.rs/):

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

### Build Commands

```bash
make build      # Build all workspace crates
make test       # Run all tests (sets RUST_MIN_STACK=16777216)
make lint       # Run clippy with -D warnings
make fmt        # Format code with rustfmt
make fmt-check  # Check formatting without modifying files
make bench      # Run benchmarks
make check      # Run fmt-check, lint, build, and test
```

> **Note:** `make test` sets `RUST_MIN_STACK=16777216` because some cryptographic
> tests (ML-DSA key operations) require a larger stack.

## Workspace Overview

| Crate | Description |
|---|---|
| `pirc-common` | Shared types, error handling, configuration |
| `pirc-protocol` | Binary wire protocol and command routing |
| `pirc-network` | Async TCP/TLS networking layer |
| `pirc-crypto` | Triple ratchet encryption, X3DH, post-quantum KEM |
| `pirc-server` | IRC server with channels, users, Raft clustering |
| `pirc-client` | Terminal UI client with multi-channel views |
| `pirc-p2p` | STUN/TURN NAT traversal and P2P group chats |
| `pirc-scripting` | mIRC-inspired scripting language and runtime |
| `pirc-plugin` | Native plugin API with C FFI |
| `tests` | End-to-end integration and performance tests |
| `examples/plugins/*` | Example plugins (hello, auto-respond, logger) |

## Code Style

- **Formatter:** rustfmt with `max_width = 100` (see `rustfmt.toml`)
- **Linter:** clippy with pedantic lints enabled (see `Cargo.toml` workspace lints)
- **Unsafe code:** denied workspace-wide
- **Tests:** inline `#[cfg(test)] mod tests` in the same file as the code under test

Run `make fmt` before committing to ensure consistent formatting.

## Making Changes

1. Create a branch from `main` for your change
2. Keep changes focused — one logical change per branch
3. Write tests for new functionality
4. Ensure `make check` passes (formatting, lints, build, tests)
5. Write a clear commit message using [conventional commits](https://www.conventionalcommits.org/):
   - `feat:` new feature
   - `fix:` bug fix
   - `docs:` documentation
   - `refactor:` code restructuring
   - `test:` test changes
   - `chore:` maintenance
6. Open a pull request against `main`

## Reporting Issues

Open an issue on GitHub with:
- Steps to reproduce
- Expected vs actual behavior
- Rust version (`rustc --version`)
- OS and platform
