# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.1] - 2026-03-02

### Added
- Crate-level rustdoc comments for `pirc-server` and `pirc-client`
- `make doc` target for generating workspace documentation via `cargo doc`
- Architecture overview and module documentation in `docs/`:
  - `docs/architecture.md` — system overview, crate graph, Raft consensus, encryption layers
  - `docs/protocol.md` — wire protocol specification, command reference, PIRC extensions, handshake sequences
  - `docs/scripting.md` — scripting DSL language reference with syntax, events, variables, and examples
  - `docs/plugins.md` — plugin development guide with C FFI interface, lifecycle, and examples
- GitHub Actions CI workflow for build, test, lint (clippy), and format check
  - Matrix strategy for Linux (ubuntu-latest) and macOS (macos-latest)
  - Rust 1.85 (MSRV) with cargo caching for faster builds
  - Stack size configured for ML-DSA key tests

## [0.1.0] - 2026-02-28

Initial release of pirc — a modern IRC client and server implementation in Rust.

### Added

#### Core Infrastructure (Phase 1)
- Workspace scaffolding with 11 crates and build system (Makefile)
- Shared types, error handling, and configuration framework (`pirc-common`)

#### Wire Protocol (Phase 2)
- Binary wire protocol with serialization, command routing, and error codes (`pirc-protocol`)
- Async TCP networking layer with TLS support and connection management (`pirc-network`)

#### Server (Phase 3)
- User and connection management with authentication
- Channel management (create, join, part, topic, modes, invites)
- Message routing, MOTD, WHO/WHOIS, and server-wide features (`pirc-server`)

#### Client TUI (Phase 4)
- Raw ANSI terminal UI engine with split-pane layout (`pirc-client`)
- Input line editing, command parsing, and history
- Multi-channel views with independent scrollback
- Full client-server connection lifecycle (connect, register, reconnect)

#### Distributed Systems (Phase 5)
- Raft consensus engine with leader election and log replication
- Cluster formation via invite keys with timing-safe validation
- State replication for channels, users, and topics across nodes
- Automatic user migration on node failure

#### Encryption (Phase 6)
- Triple ratchet encryption core with post-quantum key encapsulation (`pirc-crypto`)
- X3DH key exchange and identity verification (ML-DSA signatures)
- End-to-end encrypted private messages with forward secrecy

#### P2P Networking (Phase 7)
- STUN/TURN NAT traversal with relay fallback (`pirc-p2p`)
- P2P encrypted group chats with multi-party key agreement

#### Scripting DSL (Phase 8)
- mIRC-inspired scripting language with parser and AST (`pirc-scripting`)
- Script interpreter and runtime with event hooks and timers

#### Plugin System (Phase 9)
- Native plugin API with C FFI ABI (`pirc-plugin`)
- Dynamic plugin loading, lifecycle management, and sandboxing
- Example plugins: hello, auto-respond, logger

#### Testing & Performance (Phase 10)
- End-to-end integration test suite
- Performance optimization and OWASP-style security audit
- CI/CD pipeline and project documentation

[Unreleased]: https://github.com/niclaslindstedt/pirc/compare/v0.1.1...HEAD
[0.1.1]: https://github.com/niclaslindstedt/pirc/compare/v0.1.0...v0.1.1
