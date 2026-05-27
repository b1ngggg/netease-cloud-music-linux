#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP_ID="io.github.b1ngggg.netease_cloud_music_linux"
APP_BRANCH="stable"
MANIFEST="$ROOT_DIR/packaging/flatpak/$APP_ID.local.yml"
BUILD_DIR="$ROOT_DIR/_build/flatpak-build"
REPO_DIR="$ROOT_DIR/_build/flatpak-repo"
BUNDLE_DIR="$ROOT_DIR/_build/flatpak"
BUNDLE="$BUNDLE_DIR/netease-cloud-music-linux.flatpak"
CARGO_CACHE_DIR="$ROOT_DIR/_build/flatpak-cargo-cache"
CACHED_MANIFEST="$BUNDLE_DIR/$APP_ID.cached.yml"

if ! command -v flatpak-builder >/dev/null 2>&1; then
    echo "flatpak-builder is required. Install it with: sudo apt install flatpak-builder" >&2
    exit 1
fi

if ! flatpak --user remotes --columns=name | grep -qx "flathub"; then
    flatpak --user remote-add --if-not-exists flathub https://flathub.org/repo/flathub.flatpakrepo
fi

mkdir -p "$BUNDLE_DIR" "$CARGO_CACHE_DIR"

if [ ! -d "$CARGO_CACHE_DIR/registry" ] && [ -d "$ROOT_DIR/.flatpak-builder/build" ]; then
    LATEST_CACHE="$(find "$ROOT_DIR/.flatpak-builder/build" -maxdepth 3 -type d -name cargo | sort -V | tail -n 1 || true)"
    if [ -n "${LATEST_CACHE:-}" ]; then
        cp -a "$LATEST_CACHE/." "$CARGO_CACHE_DIR/"
    fi
fi

# b1ngggg: Keep Cargo's registry/git cache outside the transient Flatpak build dir.
python3 - "$MANIFEST" "$CACHED_MANIFEST" "$CARGO_CACHE_DIR" <<'PY'
import sys
from pathlib import Path

manifest = Path(sys.argv[1])
output = Path(sys.argv[2])
cargo_cache = sys.argv[3]

lines = manifest.read_text(encoding="utf-8").splitlines()
result = []
inserted_filesystem = False
in_build_args = False

for line in lines:
    if line.strip().startswith("CARGO_HOME:"):
        result.append(f"    CARGO_HOME: {cargo_cache}")
        continue

    result.append(line)

    stripped = line.strip()
    if stripped == "build-args:":
        in_build_args = True
        continue

    if in_build_args and stripped.startswith("- "):
        filesystem_arg = f"    - --filesystem={cargo_cache}"
        if not inserted_filesystem:
            result.append(filesystem_arg)
            inserted_filesystem = True
        in_build_args = False

if in_build_args and not inserted_filesystem:
    result.append(f"    - --filesystem={cargo_cache}")

output.write_text("\n".join(result) + "\n", encoding="utf-8")
PY

flatpak-builder \
    --user \
    --force-clean \
    --install-deps-from=flathub \
    --repo="$REPO_DIR" \
    "$BUILD_DIR" \
    "$CACHED_MANIFEST"

flatpak build-bundle "$REPO_DIR" "$BUNDLE" "$APP_ID" "$APP_BRANCH"

echo "Flatpak bundle generated:"
echo "$BUNDLE"
