#!/bin/bash
set -euo pipefail

REPO="niclaslindstedt/pirc"
CHECK_INTERVAL="${CHECK_INTERVAL:-3600}"
GRACE_PERIOD="${GRACE_PERIOD:-90}"
INTER_NODE_PAUSE="${INTER_NODE_PAUSE:-60}"
COMPOSE_PROJECT_NAME="${COMPOSE_PROJECT_NAME:-pirc}"

# Nodes updated in reverse Raft priority order: followers first, leader last.
# Lower node number = more likely to be leader (lower hash = shorter timeout).
NODES="pircd-3 pircd-2 pircd-1"

log() { echo "[updater] $(date -u '+%Y-%m-%dT%H:%M:%SZ') $*"; }

# ── get container name ────────────────────────────────────────────────────────

container_name() {
    echo "${COMPOSE_PROJECT_NAME}-${1}-1"
}

# ── fetch latest pircd version from GitHub releases ──────────────────────────

fetch_latest_version() {
    curl -fsSL "https://api.github.com/repos/${REPO}/releases" | \
        python3 -c '
import sys, json
releases = json.load(sys.stdin)
for r in releases:
    tag = r.get("tag_name", "")
    assets = r.get("assets", [])
    has_pircd = any("pircd" in a.get("name", "") for a in assets)
    if has_pircd:
        ver = tag
        for prefix in ("pircd-v", "v"):
            if ver.startswith(prefix):
                ver = ver[len(prefix):]
                break
        if ver:
            print(ver)
            break
'
}

# ── read current version from a running container ────────────────────────────

get_container_version() {
    local service="$1"
    local cname
    cname=$(container_name "$service")
    docker exec "${cname}" cat /data/current_version 2>/dev/null || echo ""
}

# ── wait for a container to become healthy ────────────────────────────────────

wait_healthy() {
    local service="$1"
    local cname
    cname=$(container_name "$service")
    local timeout=300
    local elapsed=0

    log "Waiting for ${service} to become healthy..."
    while [ "${elapsed}" -lt "${timeout}" ]; do
        local status
        status=$(docker inspect --format='{{.State.Health.Status}}' "${cname}" 2>/dev/null || echo "missing")
        case "$status" in
            healthy)
                log "${service} is healthy"
                return 0
                ;;
            missing|"")
                log "WARNING: ${service} container not found yet, retrying..."
                ;;
        esac
        sleep 5
        elapsed=$((elapsed + 5))
    done

    log "ERROR: ${service} did not become healthy within ${timeout}s"
    return 1
}

# ── perform rolling update ────────────────────────────────────────────────────

rolling_update() {
    local new_version="$1"
    log "Starting rolling update to v${new_version}"

    for service in $NODES; do
        local cname
        cname=$(container_name "$service")

        log "Updating ${service} (${cname})..."

        # Remove the version file so the entrypoint fetches the latest binary on restart.
        # The binary itself can be deleted safely while pircd is running (Linux keeps the
        # inode open until all file descriptors close).
        docker exec "${cname}" rm -f /data/current_version 2>/dev/null || true

        # Stop the container with the configured grace period.
        # This sends SIGTERM to pircd, triggering graceful shutdown (~10s user migration),
        # then SIGKILL after GRACE_PERIOD seconds if pircd has not exited.
        docker stop --time="${GRACE_PERIOD}" "${cname}" || {
            log "WARNING: failed to stop ${cname}, it may have already exited"
        }

        # Docker Compose restart policy (unless-stopped) brings the container back up.
        # The entrypoint detects a missing version file and downloads the latest binary.
        #
        # Wait for the container to restart and report healthy.
        wait_healthy "$service" || {
            log "ERROR: ${service} failed to come back healthy — aborting update"
            return 1
        }

        local deployed_version
        deployed_version=$(get_container_version "$service")
        log "${service} running v${deployed_version}"

        if [ "${service}" != "pircd-1" ]; then
            log "Pausing ${INTER_NODE_PAUSE}s before next node..."
            sleep "${INTER_NODE_PAUSE}"
        fi
    done

    log "Rolling update to v${new_version} complete"
}

# ── determine known version (from first available node) ──────────────────────

get_known_version() {
    for service in pircd-1 pircd-2 pircd-3; do
        local ver
        ver=$(get_container_version "$service")
        if [ -n "$ver" ]; then
            echo "$ver"
            return
        fi
    done
    echo ""
}

# ── main loop ─────────────────────────────────────────────────────────────────

main() {
    log "Updater started (check interval: ${CHECK_INTERVAL}s, grace period: ${GRACE_PERIOD}s)"

    # Record the currently running version on startup.
    local known_version=""
    known_version=$(get_known_version)
    log "Current version: ${known_version:-unknown}"

    while true; do
        sleep "${CHECK_INTERVAL}"

        log "Checking for new pircd releases..."
        local latest_version
        latest_version=$(fetch_latest_version) || {
            log "WARNING: failed to fetch latest version, will retry next interval"
            continue
        }

        if [ -z "${latest_version}" ]; then
            log "WARNING: could not parse latest version from GitHub API"
            continue
        fi

        log "Latest: v${latest_version}, known: ${known_version:-unknown}"

        if [ "${latest_version}" = "${known_version}" ]; then
            log "Already up to date"
            continue
        fi

        log "New version detected: v${latest_version}"
        if rolling_update "${latest_version}"; then
            known_version="${latest_version}"
            log "Update complete — now running v${known_version}"
        else
            log "ERROR: rolling update failed, will retry on next check"
        fi
    done
}

main "$@"
