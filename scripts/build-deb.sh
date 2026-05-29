#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

export PATH="$HOME/.cargo/bin:$PATH"
export PKG_CONFIG_PATH="$ROOT_DIR/_vendor/gstreamer-bad-dev/usr/lib/x86_64-linux-gnu/pkgconfig:$ROOT_DIR/_vendor/gstreamer-good-dev/usr/lib/x86_64-linux-gnu/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
export RUSTFLAGS="-L native=$ROOT_DIR/_vendor/gstreamer-bad-dev/usr/lib/x86_64-linux-gnu -L native=$ROOT_DIR/_vendor/gstreamer-good-dev/usr/lib/x86_64-linux-gnu"
export CARGO_NET_GIT_FETCH_WITH_CLI=true
export CARGO_TARGET_DIR="$ROOT_DIR/_build/target"

if ! command -v cargo-deb >/dev/null 2>&1; then
    echo "cargo-deb is required. Install it with: cargo install cargo-deb" >&2
    exit 1
fi

if [ ! -d "$ROOT_DIR/_build" ]; then
    meson setup "$ROOT_DIR/_build" --prefix=/usr --buildtype=release
else
    meson setup "$ROOT_DIR/_build" --prefix=/usr --buildtype=release --reconfigure
fi

ninja -C "$ROOT_DIR/_build"

cargo deb \
    --manifest-path "$ROOT_DIR/Cargo.toml" \
    --no-build

echo "Deb package:"
find "$ROOT_DIR/_build/target/debian" -maxdepth 1 -type f -name '*.deb' -print
