#!/usr/bin/env bash
set -euo pipefail

# Always run from repo root
cd "$(git rev-parse --show-toplevel)"

VERSION="${1:-$(grep '^version' Cargo.toml | head -1 | sed 's/.*= *"//' | sed 's/"//')}"
DIST="dist"

echo "==> Building release binaries for v${VERSION}"
mkdir -p "$DIST"

EXTRAS=()
[ -f LICENSE ] && EXTRAS+=("LICENSE")
[ -f README.md ] && EXTRAS+=("README.md")

package() {
  local bin="$1" target="$2" ext="${3:-}"
  local archive="${DIST}/${bin}-v${VERSION}-${target}.tar.gz"
  for f in "${EXTRAS[@]}"; do cp "$f" "target/${target}/release/"; done
  tar -czf "$archive" \
    -C "target/${target}/release" "${bin}${ext}" "${EXTRAS[@]}"
  echo "  Created $archive"
}

package_zip() {
  local bin="$1" target="$2" ext="${3:-.exe}"
  local archive="${DIST}/${bin}-v${VERSION}-${target}.zip"
  for f in "${EXTRAS[@]}"; do cp "$f" "target/${target}/release/"; done
  (cd "target/${target}/release" && zip -q "../../../${archive}" "${bin}${ext}" "${EXTRAS[@]}")
  echo "  Created $archive"
}

# ── macOS (native) ──────────────────────────────────────────────────────────
echo ""
echo "==> macOS aarch64"
cargo build --release --bin pirc --bin pircd --target aarch64-apple-darwin
strip "target/aarch64-apple-darwin/release/pirc"
strip "target/aarch64-apple-darwin/release/pircd"
package pirc  aarch64-apple-darwin
package pircd aarch64-apple-darwin

echo ""
echo "==> macOS x86_64"
cargo build --release --bin pirc --bin pircd --target x86_64-apple-darwin
strip "target/x86_64-apple-darwin/release/pirc"
strip "target/x86_64-apple-darwin/release/pircd"
package pirc  x86_64-apple-darwin
package pircd x86_64-apple-darwin

# ── Linux (cross) ───────────────────────────────────────────────────────────
echo ""
echo "==> Linux x86_64"
cross build --release --bin pirc --bin pircd --target x86_64-unknown-linux-gnu
package pirc  x86_64-unknown-linux-gnu
package pircd x86_64-unknown-linux-gnu

echo ""
echo "==> Linux aarch64"
cross build --release --bin pirc --bin pircd --target aarch64-unknown-linux-gnu
package pirc  aarch64-unknown-linux-gnu
package pircd aarch64-unknown-linux-gnu

# ── Windows (cross) ─────────────────────────────────────────────────────────
echo ""
echo "==> Windows x86_64"
cross build --release --bin pirc --bin pircd --target x86_64-pc-windows-gnu
package_zip pirc  x86_64-pc-windows-gnu
package_zip pircd x86_64-pc-windows-gnu

echo ""
echo "==> All done! Archives in ${DIST}/:"
ls -lh "$DIST/"
