#!/bin/bash
set -euo pipefail

DATA_DIR="/data"
RAFT_DIR="${DATA_DIR}/raft"
BINARY="/opt/pirc/pircd"
CONFIG_FILE="/tmp/pircd.toml"

# Environment variables (with defaults)
NODE_ID="${NODE_ID:-node-1}"
RAFT_PORT="${RAFT_PORT:-6668}"
IRC_PORT="${IRC_PORT:-6667}"
CLUSTER_BOOTSTRAP="${CLUSTER_BOOTSTRAP:-}"
CLUSTER_JOIN_ADDRESS="${CLUSTER_JOIN_ADDRESS:-}"
OPERATOR_NAME="${OPERATOR_NAME:-admin}"
OPERATOR_PASSWORD="${OPERATOR_PASSWORD:-}"
LOG_LEVEL="${LOG_LEVEL:-info}"

PIRCD_PID=""

# ── helpers ───────────────────────────────────────────────────────────────────

log() { echo "[entrypoint] $*"; }

# ── get this container's IP address ──────────────────────────────────────────

get_self_ip() {
    # Try getent (reliable in Debian with Docker DNS)
    local ip
    ip=$(getent hosts "$HOSTNAME" 2>/dev/null | awk '{ print $1; exit }')
    if [ -n "$ip" ] && [ "$ip" != "127.0.0.1" ]; then
        echo "$ip"
        return
    fi
    # Fallback: hostname -I (first non-loopback IP)
    ip=$(hostname -I 2>/dev/null | tr ' ' '\n' | grep -v '^127\.' | grep -v '^::' | head -1)
    if [ -n "$ip" ]; then
        echo "$ip"
        return
    fi
    echo "0.0.0.0"
}

# ── write pircd TOML config ───────────────────────────────────────────────────

write_config() {
    local mode="$1"       # bootstrap | join | rejoin
    local invite_key="${2:-}"
    local self_ip="$3"

    mkdir -p "${RAFT_DIR}"

    # Build the optional operators section
    local oper_section=""
    if [ -n "${OPERATOR_PASSWORD}" ]; then
        oper_section="$(printf '\n[[operators]]\nname = "%s"\npassword = "%s"\n' \
            "${OPERATOR_NAME}" "${OPERATOR_PASSWORD}")"
    fi

    case "$mode" in
        bootstrap)
            cat > "${CONFIG_FILE}" <<TOML
log_level = "${LOG_LEVEL}"

[network]
bind_address = "${self_ip}"
port = ${IRC_PORT}

[cluster]
enabled = true
node_id = "${NODE_ID}"
raft_port = ${RAFT_PORT}
data_dir = "${RAFT_DIR}"
bootstrap = true
${oper_section}
TOML
            ;;
        join)
            cat > "${CONFIG_FILE}" <<TOML
log_level = "${LOG_LEVEL}"

[network]
bind_address = "${self_ip}"
port = ${IRC_PORT}

[cluster]
enabled = true
raft_port = ${RAFT_PORT}
data_dir = "${RAFT_DIR}"
invite_key = "${invite_key}"
join_address = "${CLUSTER_JOIN_ADDRESS}"
${oper_section}
TOML
            ;;
        rejoin)
            # node_id here is only to satisfy the config validator;
            # the actual node ID is read from /data/raft/cluster_state.json at runtime.
            cat > "${CONFIG_FILE}" <<TOML
log_level = "${LOG_LEVEL}"

[network]
bind_address = "${self_ip}"
port = ${IRC_PORT}

[cluster]
enabled = true
node_id = "${NODE_ID}"
raft_port = ${RAFT_PORT}
data_dir = "${RAFT_DIR}"
${oper_section}
TOML
            ;;
    esac

    log "Config written (mode=${mode}, ip=${self_ip})"
}

# ── generate invite key via IRC operator command ──────────────────────────────

generate_invite_key() {
    log "Waiting for pircd to accept connections on port ${IRC_PORT}..."
    local retries=30
    while [ "$retries" -gt 0 ]; do
        if nc -z localhost "${IRC_PORT}" 2>/dev/null; then
            break
        fi
        sleep 2
        retries=$((retries - 1))
    done

    if ! nc -z localhost "${IRC_PORT}" 2>/dev/null; then
        log "ERROR: pircd did not become ready in time"
        return 1
    fi

    log "Generating invite key via IRC operator command..."
    python3 - <<PYEOF
import socket, sys, os

host = "127.0.0.1"
port = int(os.environ.get("IRC_PORT", "6667"))
nick = "pirc-bootstrap"
op_name = os.environ.get("OPERATOR_NAME", "admin")
op_pass = os.environ.get("OPERATOR_PASSWORD", "")


def send(s, line):
    s.sendall((line + "\r\n").encode())


def recv_lines(s, timeout=3):
    s.settimeout(timeout)
    buf = b""
    lines = []
    try:
        while True:
            chunk = s.recv(4096)
            if not chunk:
                break
            buf += chunk
            while b"\r\n" in buf:
                line, buf = buf.split(b"\r\n", 1)
                lines.append(line.decode("utf-8", errors="replace"))
    except socket.timeout:
        pass
    return lines


sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
sock.connect((host, port))

send(sock, f"NICK {nick}")
send(sock, f"USER bootstrap 0 * :Bootstrap Agent")

# Wait for 001 (welcome), reply to any PING during registration
registered = False
for _ in range(60):
    lines = recv_lines(sock, timeout=2)
    for line in lines:
        if line.startswith("PING"):
            token = line.split(":", 1)[-1] if ":" in line else line.split(" ", 1)[-1]
            send(sock, f"PONG :{token}")
        if f" 001 {nick}" in line:
            registered = True
    if registered:
        break

if not registered:
    print("ERROR: did not receive welcome message", file=sys.stderr)
    sys.exit(1)

# Authenticate as IRC operator
send(sock, f"OPER {op_name} {op_pass}")

oper_ok = False
for _ in range(10):
    lines = recv_lines(sock, timeout=2)
    for line in lines:
        if " 381 " in line:
            oper_ok = True
        if " 491 " in line or " 464 " in line:
            print(f"ERROR: OPER authentication failed: {line}", file=sys.stderr)
            sys.exit(1)
    if oper_ok:
        break

if not oper_ok:
    print("ERROR: did not receive OPER confirmation", file=sys.stderr)
    sys.exit(1)

# Request invite key generation
send(sock, "PIRC INVITE-KEY GENERATE")

# Parse the NOTICE response containing the key
for _ in range(10):
    lines = recv_lines(sock, timeout=2)
    for line in lines:
        if "Invite key generated:" in line:
            # Format: ":server NOTICE nick :Invite key generated: KEY"
            key = line.split("Invite key generated:")[-1].strip()
            print(key)
            sock.close()
            sys.exit(0)

print("ERROR: did not receive invite key in NOTICE response", file=sys.stderr)
sys.exit(1)
PYEOF
}

# ── signal handler for bootstrap mode ────────────────────────────────────────

handle_signal() {
    log "Signal received, forwarding to pircd (PID=${PIRCD_PID})..."
    if [ -n "${PIRCD_PID}" ]; then
        kill -TERM "${PIRCD_PID}" 2>/dev/null || true
    fi
}

# ── main ──────────────────────────────────────────────────────────────────────

main() {
    mkdir -p "${DATA_DIR}" "${RAFT_DIR}"

    if [ ! -x "${BINARY}" ]; then
        log "ERROR: pircd binary not found at ${BINARY}"
        exit 1
    fi
    log "Using pircd binary at ${BINARY}"

    local self_ip
    self_ip=$(get_self_ip)
    log "Container IP: ${self_ip}"

    # Determine startup mode based on persisted state
    local cluster_state_file="${RAFT_DIR}/cluster_state.json"
    local has_state=false
    [ -f "${cluster_state_file}" ] && has_state=true

    if [ "${has_state}" = "true" ]; then
        # ── Rejoin ────────────────────────────────────────────────────────────
        # Persisted cluster state exists — rejoin without invite key.
        # The actual node_id and peer topology are read from cluster_state.json
        # at runtime; the node_id in config is only to satisfy the validator.
        log "Existing cluster state found — starting in rejoin mode"
        write_config "rejoin" "" "${self_ip}"
        exec "${BINARY}" --config "${CONFIG_FILE}"

    elif [ -n "${CLUSTER_BOOTSTRAP}" ]; then
        # ── Bootstrap ─────────────────────────────────────────────────────────
        # First boot of the bootstrap leader node.
        log "Starting pircd in bootstrap mode..."
        write_config "bootstrap" "" "${self_ip}"

        trap 'handle_signal' TERM INT

        "${BINARY}" --config "${CONFIG_FILE}" &
        PIRCD_PID=$!

        # Generate and publish the cluster invite key for joining nodes.
        if [ ! -f "/cluster-init/invite_key" ]; then
            local invite_key
            invite_key=$(generate_invite_key) || {
                log "ERROR: invite key generation failed"
                kill "${PIRCD_PID}" 2>/dev/null || true
                wait "${PIRCD_PID}" 2>/dev/null || true
                exit 1
            }
            echo "${invite_key}" > /cluster-init/invite_key
            log "Invite key written to /cluster-init/invite_key"
        else
            log "Invite key already present in /cluster-init/invite_key"
        fi

        # Stay alive until pircd exits (e.g. on SIGTERM from Docker)
        wait "${PIRCD_PID}"

    else
        # ── Join ──────────────────────────────────────────────────────────────
        # First boot of a non-bootstrap node: wait for invite key then join.
        log "Waiting for invite key from bootstrap node (timeout: 5m)..."
        local timeout=300
        local elapsed=0
        while [ ! -f "/cluster-init/invite_key" ] && [ "${elapsed}" -lt "${timeout}" ]; do
            sleep 5
            elapsed=$((elapsed + 5))
        done

        if [ ! -f "/cluster-init/invite_key" ]; then
            log "ERROR: timed out waiting for /cluster-init/invite_key"
            exit 1
        fi

        local invite_key
        invite_key=$(cat /cluster-init/invite_key)
        log "Invite key received — joining cluster at ${CLUSTER_JOIN_ADDRESS}"

        write_config "join" "${invite_key}" "${self_ip}"
        exec "${BINARY}" --config "${CONFIG_FILE}"
    fi
}

main "$@"
