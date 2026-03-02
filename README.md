# pirc

A modern, terminal-based IRC client and distributed server written in Rust. Inspired by mIRC, built for 2025: distributed server clusters with Raft consensus, post-quantum encryption, E2E encrypted messaging, P2P group chats, a custom scripting DSL, and native plugin support.

## Features

- **Terminal client** (`pirc`) with mIRC-style commands and multi-channel views
- **Dedicated server** (`pircd`) with channels, user management, operator privileges, and MOTD
- **Distributed clustering** — servers form a Raft consensus cluster with automatic leader election, state replication, and transparent user migration (no netsplits)
- **Post-quantum encryption** — triple ratchet protocol with ML-KEM key exchange, ML-DSA signatures, X25519, and AES-256-GCM
- **E2E encrypted private messages** — servers never see plaintext
- **P2P encrypted group chats** — direct client-to-client with STUN/TURN NAT traversal
- **Forward secrecy** — past messages stay safe even if long-term keys are compromised
- **Custom scripting DSL** — mIRC-inspired aliases, event handlers, timers, and variables
- **Native plugin system** — compiled Rust dynamic libraries (`.so`/`.dylib`) with a C FFI API
- **Zero-config startup** — connect with just a server address and nickname

## Installation

### Download pre-built binaries (recommended)

Install both `pirc` and `pircd` with a single command — platform is auto-detected:

```bash
curl -fsSL https://raw.githubusercontent.com/niclaslindstedt/pirc/main/scripts/install.sh | sh
```

Binaries are installed to `/usr/local/bin` (or `~/.local/bin` if that isn't writable). Supported platforms: **macOS** (Apple Silicon and Intel) and **Linux** (x86_64 and aarch64).

**Options** (pass via environment variables):

```bash
# Install a specific version
PIRC_VERSION=0.1.1 curl -fsSL ... | sh

# Install only the server daemon
BINARIES=pircd curl -fsSL ... | sh

# Install to a custom directory
INSTALL_DIR=~/.local/bin curl -fsSL ... | sh
```

Or download a specific release archive manually from the [releases page](https://github.com/niclaslindstedt/pirc/releases).

### Build from source

Requires a **Rust** stable toolchain (minimum supported version: 1.85) and **Make**.

Install Rust via [rustup](https://rustup.rs/):

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Then clone and build:

```bash
git clone https://github.com/niclaslindstedt/pirc.git
cd pirc
make build
```

This produces two binaries in `target/debug/`:

| Binary | Description |
|--------|-------------|
| `pirc` | Terminal IRC client |
| `pircd` | Dedicated IRC server |

## Quick Start

Start a server with default settings (binds to `0.0.0.0:6667`):

```bash
cargo run --bin pircd
```

Connect with the client:

```bash
cargo run --bin pirc
```

Or with a custom config:

```bash
cargo run --bin pircd -- --config path/to/pircd.toml
cargo run --bin pirc -- --config path/to/pirc.toml
```

## Usage

### Client Commands

pirc uses mIRC-style `/` commands:

| Command | Description |
|---------|-------------|
| `/join #channel` | Join a channel |
| `/part #channel` | Leave a channel |
| `/msg nick message` | Send a private message |
| `/nick newnick` | Change your nickname |
| `/topic #channel new topic` | Set channel topic |
| `/kick #channel nick` | Kick a user from a channel |
| `/ban #channel nick` | Ban a user from a channel |
| `/mode #channel +o nick` | Set channel/user modes |
| `/whois nick` | Query user information |
| `/list` | List channels |
| `/invite nick #channel` | Invite a user to a channel |
| `/away [message]` | Set away status |
| `/me action` | Send an action message |
| `/quit [message]` | Disconnect from the server |

**Operator commands:** `/oper`, `/kill`, `/die`, `/restart`, `/wallops`

### Server Clustering

To run a multi-server cluster:

1. **Bootstrap the first server** (master):

   ```toml
   # pircd.toml
   [cluster]
   enabled = true
   node_id = 1
   raft_port = 6668
   ```

2. **Generate an invite key** on the master (via operator command)

3. **Join additional servers** using the invite key:

   ```toml
   # pircd.toml (second server)
   [cluster]
   enabled = true
   invite_key = "the-generated-key"
   join_address = "master-ip:6668"
   raft_port = 6669
   ```

Servers automatically elect leaders, replicate state, and migrate users on failover.

## Configuration

Configuration files use TOML format and are auto-discovered from:

| File | Locations (in order) |
|------|---------------------|
| Client | `$XDG_CONFIG_HOME/pirc/pirc.toml`, `~/.pirc/pirc.toml` |
| Server | `$XDG_CONFIG_HOME/pirc/pircd.toml`, `~/.pirc/pircd.toml`, `/etc/pirc/pircd.toml` |

Both binaries start with sensible defaults if no config file exists.

### Key directories

| Path | Purpose |
|------|---------|
| `~/.pirc/scripts/` | Scripting DSL files |
| `~/.pirc/plugins/` | Native plugin libraries |
| `~/.pirc/keys/` | Encrypted key storage |

## Scripting

pirc includes a custom scripting language inspired by mIRC scripting. Scripts are loaded from `~/.pirc/scripts/`.

```
alias greet {
  msg $chan Hello, $nick!
}

on JOIN {
  if ($nick != $me) {
    msg $chan Welcome, $nick!
  }
}

timer 300 {
  msg #general Still here!
}
```

See [docs/scripting.md](docs/scripting.md) for the full language reference.

## Plugins

Plugins are compiled Rust dynamic libraries loaded from `~/.pirc/plugins/`. Example plugins are provided in `examples/plugins/`:

- **hello-plugin** — registers a `/hello` command
- **auto-respond-plugin** — automatically responds to messages
- **logger-plugin** — logs events to a file

See [docs/plugins.md](docs/plugins.md) for the plugin development guide.

## Development

### Build commands

```bash
make build      # Build all workspace crates
make test       # Run all tests
make lint       # Run clippy with -D warnings
make fmt        # Format code
make fmt-check  # Check formatting
make bench      # Run benchmarks
make doc        # Generate rustdoc
make check      # Run fmt-check + lint + build + test
```

### Workspace structure

| Crate | Description |
|-------|-------------|
| `pirc-common` | Shared types, error handling, configuration paths |
| `pirc-protocol` | Text-based wire protocol and command definitions |
| `pirc-network` | Async TCP networking layer |
| `pirc-crypto` | Triple ratchet encryption, X3DH, post-quantum KEM |
| `pirc-server` | Server with channels, users, Raft clustering |
| `pirc-client` | Terminal UI client |
| `pirc-p2p` | STUN/TURN NAT traversal and P2P connections |
| `pirc-scripting` | mIRC-inspired scripting language and runtime |
| `pirc-plugin` | Native plugin API with C FFI |
| `tests` | Integration and performance tests |

### Architecture

See [docs/architecture.md](docs/architecture.md) for a detailed system overview and [docs/protocol.md](docs/protocol.md) for the wire protocol specification.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for development setup, code style, and contribution guidelines.

## License

MIT
