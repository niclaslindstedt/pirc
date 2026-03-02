#!/usr/bin/env sh
# install.sh — download and install pirc and/or pircd from the latest GitHub release
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/niclaslindstedt/pirc/main/scripts/install.sh | sh
#
# Options (environment variables):
#   INSTALL_DIR   — where to install binaries (default: /usr/local/bin, falls back to ~/.local/bin)
#   PIRC_VERSION  — specific version to install, e.g. "0.1.1" (default: latest)
#   BINARIES      — space-separated list of binaries to install (default: "pirc pircd")

set -e

REPO="niclaslindstedt/pirc"
INSTALL_DIR="${INSTALL_DIR:-}"
PIRC_VERSION="${PIRC_VERSION:-}"
BINARIES="${BINARIES:-pirc pircd}"

# ── helpers ──────────────────────────────────────────────────────────────────

info()  { printf '\033[1;34m=>\033[0m %s\n' "$*"; }
ok()    { printf '\033[1;32m✓\033[0m  %s\n' "$*"; }
err()   { printf '\033[1;31mError:\033[0m %s\n' "$*" >&2; exit 1; }

need() {
  command -v "$1" >/dev/null 2>&1 || err "required tool not found: $1"
}

# ── detect OS and architecture ───────────────────────────────────────────────

detect_target() {
  os="$(uname -s)"
  arch="$(uname -m)"

  case "$os" in
    Darwin) os_part="apple-darwin" ;;
    Linux)  os_part="unknown-linux-gnu" ;;
    *)      err "unsupported OS: $os (only macOS and Linux are supported)" ;;
  esac

  case "$arch" in
    x86_64)          arch_part="x86_64" ;;
    arm64 | aarch64) arch_part="aarch64" ;;
    *)               err "unsupported architecture: $arch" ;;
  esac

  echo "${arch_part}-${os_part}"
}

# ── resolve install directory ────────────────────────────────────────────────

resolve_install_dir() {
  if [ -n "$INSTALL_DIR" ]; then
    echo "$INSTALL_DIR"
    return
  fi

  if [ -w "/usr/local/bin" ]; then
    echo "/usr/local/bin"
  elif [ -d "$HOME/.local/bin" ]; then
    echo "$HOME/.local/bin"
  else
    mkdir -p "$HOME/.local/bin"
    echo "$HOME/.local/bin"
  fi
}

# ── fetch latest version from GitHub ────────────────────────────────────────

fetch_latest_version() {
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
      | grep '"tag_name"' \
      | sed 's/.*"tag_name": *"v\([^"]*\)".*/\1/'
  elif command -v wget >/dev/null 2>&1; then
    wget -qO- "https://api.github.com/repos/${REPO}/releases/latest" \
      | grep '"tag_name"' \
      | sed 's/.*"tag_name": *"v\([^"]*\)".*/\1/'
  else
    err "curl or wget is required"
  fi
}

# ── download and extract one binary ─────────────────────────────────────────

install_binary() {
  bin="$1"
  version="$2"
  target="$3"
  dest="$4"

  archive="${bin}-v${version}-${target}.tar.gz"
  url="https://github.com/${REPO}/releases/download/v${version}/${archive}"

  info "Downloading $archive"

  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' EXIT INT TERM

  if command -v curl >/dev/null 2>&1; then
    curl -fsSL --progress-bar "$url" -o "${tmp}/${archive}" \
      || err "download failed: $url"
  else
    wget -q --show-progress "$url" -O "${tmp}/${archive}" \
      || err "download failed: $url"
  fi

  tar -xzf "${tmp}/${archive}" -C "$tmp" "$bin"

  if [ -w "$dest" ]; then
    mv "${tmp}/${bin}" "${dest}/${bin}"
    chmod +x "${dest}/${bin}"
  else
    info "Installing to $dest (sudo required)"
    sudo mv "${tmp}/${bin}" "${dest}/${bin}"
    sudo chmod +x "${dest}/${bin}"
  fi

  trap - EXIT INT TERM
  rm -rf "$tmp"

  ok "Installed ${bin} → ${dest}/${bin}"
}

# ── main ─────────────────────────────────────────────────────────────────────

main() {
  need grep
  need sed
  need tar

  target="$(detect_target)"
  info "Detected platform: $target"

  if [ -z "$PIRC_VERSION" ]; then
    info "Fetching latest release version..."
    PIRC_VERSION="$(fetch_latest_version)"
    [ -n "$PIRC_VERSION" ] || err "could not determine latest version"
  fi
  info "Version: v${PIRC_VERSION}"

  dest="$(resolve_install_dir)"
  info "Install directory: $dest"

  for bin in $BINARIES; do
    install_binary "$bin" "$PIRC_VERSION" "$target" "$dest"
  done

  echo ""
  ok "Done! Make sure $dest is in your PATH."

  case ":${PATH}:" in
    *":${dest}:"*) ;;
    *)
      printf '\033[1;33mNote:\033[0m Add the following to your shell profile if needed:\n'
      printf '  export PATH="%s:$PATH"\n' "$dest"
      ;;
  esac
}

main "$@"
