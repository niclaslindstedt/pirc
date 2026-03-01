#!/usr/bin/env bash
set -euo pipefail

# Usage: ./scripts/release.sh [major|minor|patch] [--dry-run]
# Default bump type: patch

BUMP="${1:-patch}"
DRY_RUN=false

for arg in "$@"; do
    if [[ "$arg" == "--dry-run" ]]; then
        DRY_RUN=true
    fi
done

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

# ── Validation ────────────────────────────────────────────────────────────────

if ! command -v gh &>/dev/null; then
    echo "error: 'gh' CLI is not installed or not in PATH" >&2
    exit 1
fi

if ! command -v python3.12 &>/dev/null; then
    echo "error: python3.12 is not installed or not in PATH" >&2
    exit 1
fi

CURRENT_BRANCH="$(git rev-parse --abbrev-ref HEAD)"
if [[ "$CURRENT_BRANCH" != "main" ]]; then
    echo "error: must be on 'main' branch (currently on '$CURRENT_BRANCH')" >&2
    exit 1
fi

if [[ -n "$(git status --porcelain)" ]]; then
    echo "error: working tree is not clean — commit or stash your changes first" >&2
    exit 1
fi

if [[ "$BUMP" != "major" && "$BUMP" != "minor" && "$BUMP" != "patch" ]]; then
    echo "error: bump type must be 'major', 'minor', or 'patch' (got '$BUMP')" >&2
    exit 1
fi

# ── Version computation ───────────────────────────────────────────────────────

CURRENT_VERSION="$(grep -m1 '^version = ' Cargo.toml | sed 's/version = "\(.*\)"/\1/')"

NEW_VERSION="$(python3.12 - "$CURRENT_VERSION" "$BUMP" <<'EOF'
import sys
parts = sys.argv[1].split(".")
major, minor, patch = int(parts[0]), int(parts[1]), int(parts[2])
bump = sys.argv[2]
if bump == "major":
    major += 1; minor = 0; patch = 0
elif bump == "minor":
    minor += 1; patch = 0
else:
    patch += 1
print(f"{major}.{minor}.{patch}")
EOF
)"

echo "Releasing: $CURRENT_VERSION → $NEW_VERSION (bump: $BUMP)"
if $DRY_RUN; then
    echo "[dry-run] skipping git/push steps"
fi

# ── Update Cargo.toml ─────────────────────────────────────────────────────────

sed -i.bak "s/^version = \"${CURRENT_VERSION}\"/version = \"${NEW_VERSION}\"/" Cargo.toml
rm -f Cargo.toml.bak

echo "Updated Cargo.toml: $CURRENT_VERSION → $NEW_VERSION"

# ── Update Cargo.lock ─────────────────────────────────────────────────────────

cargo build --workspace --quiet
echo "Updated Cargo.lock"

# ── Update CHANGELOG.md ───────────────────────────────────────────────────────

TODAY="$(date +%Y-%m-%d)"

python3.12 - "$CURRENT_VERSION" "$NEW_VERSION" "$TODAY" <<'EOF'
import sys, re, pathlib

prev_version = sys.argv[1]
new_version  = sys.argv[2]
today        = sys.argv[3]

changelog = pathlib.Path("CHANGELOG.md")
text = changelog.read_text()

# Extract the [Unreleased] section body
unreleased_pattern = re.compile(
    r'(## \[Unreleased\]\n)(.*?)(?=\n## \[|\Z)',
    re.DOTALL
)
m = unreleased_pattern.search(text)
if not m:
    print("error: could not find [Unreleased] section in CHANGELOG.md", file=sys.stderr)
    sys.exit(1)

unreleased_body = m.group(2).rstrip('\n')

# Build replacement: fresh [Unreleased] + new versioned section
new_section = (
    f"## [Unreleased]\n\n"
    f"## [{new_version}] - {today}\n"
    f"{unreleased_body}\n"
)

text = unreleased_pattern.sub(new_section, text, count=1)

# Update or append link references at the bottom
unreleased_link = f"[Unreleased]: https://github.com/niclaslindstedt/pirc/compare/v{new_version}...HEAD"
version_link    = f"[{new_version}]: https://github.com/niclaslindstedt/pirc/compare/v{prev_version}...v{new_version}"

# Replace existing [Unreleased] link reference if present
if re.search(r'^\[Unreleased\]:', text, re.MULTILINE):
    text = re.sub(r'^\[Unreleased\]:.*$', unreleased_link, text, flags=re.MULTILINE)
else:
    text = text.rstrip('\n') + '\n\n' + unreleased_link + '\n'

# Append new version link reference
text = text.rstrip('\n') + '\n' + version_link + '\n'

changelog.write_text(text)
print(f"Updated CHANGELOG.md: added [{new_version}] section dated {today}")
EOF

# ── Git commit, tag, push ─────────────────────────────────────────────────────

if $DRY_RUN; then
    echo "[dry-run] would run:"
    echo "  git add Cargo.toml Cargo.lock CHANGELOG.md"
    echo "  git commit -m 'chore: release v${NEW_VERSION}'"
    echo "  git tag -a 'v${NEW_VERSION}' -m 'Release v${NEW_VERSION}'"
    echo "  git push origin main --follow-tags"
    echo ""
    echo "Dry run complete. Reverting Cargo.toml changes..."
    git checkout Cargo.toml Cargo.lock CHANGELOG.md 2>/dev/null || true
    exit 0
fi

git add Cargo.toml Cargo.lock CHANGELOG.md
git commit -m "chore: release v${NEW_VERSION}"
git tag -a "v${NEW_VERSION}" -m "Release v${NEW_VERSION}"
git push origin main --follow-tags

echo ""
echo "Released v${NEW_VERSION}. GitHub Actions will now build and publish the release."
echo "Track progress at: https://github.com/niclaslindstedt/pirc/actions"
