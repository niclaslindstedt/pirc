# Architecture Overview

pirc is a modern IRC client and server implementation in Rust, featuring post-quantum encryption, Raft-based server clustering, P2P connectivity, a scripting DSL, and a native plugin system.

## System Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│                         pirc Network                                │
│                                                                     │
│  ┌──────────┐    TCP    ┌──────────┐  Raft RPC  ┌──────────┐      │
│  │  Client   │◄────────►│  Server  │◄──────────►│  Server  │      │
│  │  (pirc)   │          │  (pircd) │            │  (pircd) │      │
│  └──────────┘          └──────────┘            └──────────┘      │
│       │                     ▲                       ▲              │
│       │ P2P (UDP)           │ TCP                   │              │
│       ▼                     ▼                       │              │
│  ┌──────────┐         ┌──────────┐                  │              │
│  │  Client   │         │  Server  │◄─────────────────┘              │
│  │  (pirc)   │         │  (pircd) │  Raft RPC                      │
│  └──────────┘         └──────────┘                                 │
└─────────────────────────────────────────────────────────────────────┘
```

### Communication Patterns

- **Client ↔ Server:** TCP connections using the IRC wire protocol (text-based, `\r\n` delimited, 512-byte max per message). Clients connect to any server in the cluster.
- **Server ↔ Server:** Raft consensus RPCs for state replication. The leader coordinates all cluster state changes (user registrations, channel operations, topic changes).
- **Client ↔ Client (P2P):** Direct UDP connections established via ICE/STUN/TURN NAT traversal. Used for encrypted direct messaging and group chats, bypassing the server for message content.

## Crate Dependency Graph

```
pirc-common          Shared types, errors, configuration paths
    │
    ├── pirc-protocol    Wire protocol: message parsing, commands, numeric replies
    │       │
    │       └── pirc-network     Async TCP transport, framing, connection pooling
    │
    ├── pirc-crypto      Cryptographic primitives: X3DH, triple ratchet, ML-KEM, ML-DSA
    │
    ├── pirc-scripting   mIRC-inspired scripting DSL: lexer, parser, interpreter
    │
    ├── pirc-plugin      Native plugin system: C FFI, dynamic loading, sandboxing
    │
    └── pirc-p2p         P2P connectivity: STUN, TURN, ICE, encrypted transport
```

**Binaries:**
- `pirc-client` → `pirc` — TUI client (depends on all crates above)
- `pirc-server` → `pircd` — Server daemon (depends on common, protocol, network, crypto)

## Crate Responsibilities

### pirc-common

Foundation crate providing validated IRC types used across the entire workspace.

- **Validated types:** `Nickname` (case-insensitive equality/hashing), `ChannelName` (must start with `#` or `&`), `ServerId`, `UserId`, `GroupId`
- **Mode types:** `ChannelMode` (InviteOnly, Moderated, Secret, Topic, NoExternal, Private), `UserMode` (Away, Invisible, Wallops, Operator)
- **Error hierarchy:** `PircError` as the top-level error with domain-specific variants (`ChannelError`, `UserError`, `RaftError`, `InviteKeyError`)
- **Configuration paths:** XDG-compatible directory resolution (`config_dir()`, `keys_dir()`, `plugins_dir()`, `scripts_dir()`)

### pirc-protocol

Defines the text-based IRC wire protocol with pirc-specific extensions.

- **Message format:** `:<prefix> <command> <params...> :<trailing>\r\n` (RFC 2812 compatible)
- **Command enum:** Standard IRC commands (NICK, JOIN, PRIVMSG, etc.) plus `PIRC` subcommands for encryption, clustering, P2P, groups, and invite keys
- **Parser:** Validates and parses wire-format messages (512-byte limit, max 15 parameters)
- **Numeric replies:** Standard IRC numeric codes (RPL_WELCOME, ERR_NICKNAMEINUSE, etc.)
- **Builder:** Fluent `MessageBuilder` API for constructing messages

### pirc-network

Async TCP networking layer built on Tokio.

- **Transport trait:** `AsyncTransport` — abstract interface for send/recv/shutdown with socket address access
- **Connection:** TCP stream with `PircCodec` for `\r\n` framing and message parsing
- **Listener/Connector:** Accept inbound and establish outbound connections with reconnection policies
- **Connection pool:** Manages server-to-server links with insert/remove/lookup by peer ID
- **Backpressure:** `BackpressureController` with bounded channels, read rate limiting, and write buffer configuration
- **Shutdown:** Coordinated graceful shutdown with message flush

### pirc-crypto

Post-quantum-resistant end-to-end encryption using a triple ratchet protocol.

- **Triple ratchet:** Three layers of key evolution operating at different timescales:
  1. **Symmetric ratchet** — per-message key derivation (~7µs encrypt, ~4µs decrypt)
  2. **DH ratchet** — X25519 Diffie-Hellman for break-in recovery per round
  3. **PQ ratchet** — ML-KEM (Kyber) key encapsulation every 20 DH steps for post-quantum resistance
- **X3DH:** Extended Triple Diffie-Hellman for initial session setup with PQ extension
- **Signing:** ML-DSA (Dilithium) digital signatures for identity verification (~598µs sign, ~32µs verify)
- **AEAD:** AES-256-GCM authenticated encryption (~2µs for 60-byte messages)
- **Key storage:** Password-protected encrypted-at-rest key store (Argon2id KDF)
- **Header encryption:** Separates metadata from payload for robust out-of-order message handling (skipped key cache up to 1000 keys)

### pirc-server

Full IRC server with Raft-based distributed consensus.

- **User registry:** Thread-safe nickname→session mapping via DashMap (case-insensitive)
- **Channel registry:** Channel state with topics, modes, member lists, bans, invites
- **Command handlers:** Modular handlers for channel ops, operator commands, groups, P2P relay, and server-to-server relay
- **Offline store:** Queues messages for offline users
- **Pre-key store:** Stores X3DH pre-key bundles for key exchange
- **Raft consensus** (see below)
- **Cluster service:** Manages membership with signed invite keys

### pirc-client

Terminal-based IRC client with TUI.

- **TUI:** Raw ANSI terminal engine with split-pane layout, input editing, command history
- **Multi-channel views:** Independent scrollback per channel
- **Encryption integration:** X3DH key exchange and triple ratchet for E2E encrypted messaging
- **P2P sessions:** Direct peer connections for private and group messaging
- **Script engine:** Loads and executes `.pirc` scripts from `~/.pirc/scripts/`
- **Plugin loader:** Loads native plugins from `~/.pirc/plugins/`

### pirc-p2p

P2P connectivity with NAT traversal.

- **STUN:** RFC 5389 binding requests to discover server-reflexive addresses
- **TURN:** RFC 5766 relay allocation when direct connectivity fails
- **ICE:** Candidate gathering (host, server-reflexive, relay) with trickle ICE support
- **Connectivity checks:** STUN-based probing of candidate pairs with state tracking
- **Session state machine:** Idle → GatheringCandidates → Offer/Answer → Checking → Connected/Failed (5-second timeout)
- **Group mesh:** Full N×(N-1)/2 mesh topology for multi-peer groups
- **Encrypted transport:** Symmetric cipher over UDP frames

### pirc-scripting

mIRC-inspired domain-specific language for client automation.

- **Language features:** Aliases, event handlers, timers, variables (local `%var` / global `%%var`), control flow (if/while), expressions, string interpolation, regex matching
- **Script engine:** Load/unload/reload scripts, dispatch events, execute aliases, tick timers
- **Host interface:** `ScriptHost` trait for callbacks to the client (send commands, echo output, get state)
- **Built-in identifiers:** `$me`, `$chan`, `$server`, `$port`, `$nick`, `$1`-`$9` (regex captures)

### pirc-plugin

Native plugin system with C FFI.

- **Plugin trait:** `init()`, `shutdown()`, `handle_event()`, `handle_command()`
- **Capability sandbox:** Plugins declare required permissions (`ReadConfig`, `WriteConfig`, `AccessChat`, `RegisterCommand`, etc.)
- **Dynamic loading:** `libloading`-based `.dylib`/`.so` loader with C ABI vtable
- **`declare_plugin!` macro:** Generates FFI bridge automatically
- **Plugin manager:** Lifecycle coordination, event forwarding, command routing
- **Example plugins:** hello, auto-respond, logger

## Raft Consensus

The server cluster uses Raft for distributed consensus, ensuring consistent state across all nodes.

### Components

```
┌─────────────────────────────────────────────┐
│                RaftDriver                    │
│  (async task: orchestrates node + network)   │
│                                              │
│  ┌────────────┐    ┌──────────────────────┐ │
│  │  RaftNode   │    │  PeerConnections     │ │
│  │  (state     │    │  (outbound TCP to    │ │
│  │   machine)  │    │   other servers)     │ │
│  └─────┬──────┘    └──────────────────────┘ │
│        │                                     │
│  ┌─────▼──────┐    ┌──────────────────────┐ │
│  │  RaftLog    │    │  ElectionTracker     │ │
│  │  (in-memory │    │  (vote counting)     │ │
│  │   + storage)│    │                      │ │
│  └─────┬──────┘    └──────────────────────┘ │
│        │                                     │
│  ┌─────▼──────┐    ┌──────────────────────┐ │
│  │ FileStorage │    │  ClusterStateMachine │ │
│  │ (persistent │    │  (apply commands,    │ │
│  │  disk store)│    │   produce snapshots) │ │
│  └────────────┘    └──────────────────────┘ │
└─────────────────────────────────────────────┘
```

### State Transitions

```
             start_election()
Follower ──────────────────────► Candidate
    ▲                                │
    │        election timeout        │ majority votes
    │        or higher term          │
    │                                ▼
    └──────────────────────────── Leader
              higher term
```

### Key Design Decisions

- **Transport-agnostic core:** `RaftNode` produces outbound messages via an `mpsc::UnboundedSender`, decoupling consensus logic from networking.
- **Deterministic election timeout:** Lower node IDs get shorter timeouts, creating a stable succession order that reduces election contention.
- **Snapshot compaction:** Log entries are compacted into snapshots (default threshold: 1000 entries) and transferred in 64KB chunks.
- **Health monitoring:** `NodeHealthMonitor` tracks peer responsiveness with Healthy/Degraded/Unreachable states.
- **Storage trait:** Async `RaftStorage<T>` trait allows pluggable backends (production uses `FileStorage`, tests use in-memory `MemStorage`).

### RPC Messages

| Message | Purpose |
|---------|---------|
| `RequestVote` | Candidate requests votes during election |
| `AppendEntries` | Leader replicates log entries / heartbeat |
| `InstallSnapshot` | Leader sends state snapshot to lagging follower |

### Cluster State Machine

The `ClusterStateMachine` applies replicated commands to maintain consistent state across the cluster. Commands include user registrations, channel operations, topic changes, and membership updates. The state machine supports snapshotting and restoration for efficient log compaction.

## Encryption Layers

```
┌─────────────────────────────────────────────┐
│              Application Layer               │
│         (plaintext IRC messages)             │
├─────────────────────────────────────────────┤
│           Triple Ratchet Session             │
│  ┌─────────────┬──────────┬──────────────┐  │
│  │  Symmetric   │    DH    │     PQ       │  │
│  │  Ratchet     │  Ratchet │   Ratchet    │  │
│  │  (per-msg)   │ (per-rnd)│ (periodic)   │  │
│  │  HKDF-SHA256 │  X25519  │   ML-KEM     │  │
│  └─────────────┴──────────┴──────────────┘  │
├─────────────────────────────────────────────┤
│              AEAD (AES-256-GCM)              │
│      (authenticated ciphertext + header)     │
├─────────────────────────────────────────────┤
│            Transport (TCP or UDP)             │
└─────────────────────────────────────────────┘
```

### Session Establishment

1. **X3DH key exchange** with post-quantum extension establishes a shared secret between two parties.
2. **Triple ratchet initialization** derives initial root, chain, and header keys from the shared secret.
3. **Ongoing communication** uses the symmetric ratchet for per-message keys, with DH and PQ ratchets providing forward secrecy and post-quantum resistance.

### Identity Verification

Long-term identity keys use ML-DSA (Dilithium) digital signatures. Users can verify each other's fingerprints out-of-band to confirm identity.

## Directory Layout

```
pirc/
├── pirc-common/         Shared types, errors, config
├── pirc-protocol/       Wire protocol definition
├── pirc-network/        Async TCP networking
├── pirc-crypto/         Cryptographic primitives
├── pirc-server/         Server binary (pircd)
├── pirc-client/         Client binary (pirc)
├── pirc-p2p/            P2P connectivity
├── pirc-scripting/      Scripting DSL
├── pirc-plugin/         Plugin system
├── examples/
│   ├── hello-plugin/    Minimal plugin example
│   ├── auto-respond-plugin/
│   └── logger-plugin/
├── tests/               End-to-end tests
├── docs/                Documentation
│   └── decisions/       Architecture Decision Records
├── Cargo.toml           Workspace manifest
├── Makefile             Build, test, lint commands
└── .github/workflows/   CI/CD pipeline
```

## Build System

```bash
make build    # Compile all crates
make test     # Run test suite (sets RUST_MIN_STACK for crypto tests)
make lint     # Clippy with -D warnings (pedantic enabled)
```

**Minimum Supported Rust Version (MSRV):** 1.85

**Platforms:** Linux (ubuntu-latest), macOS (macos-latest)
