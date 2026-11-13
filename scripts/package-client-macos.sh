#!/usr/bin/env bash
# Cross-compile the gilfoyle client for macOS from Linux, and package it as one tarball per
# architecture plus a checksum, in the layout of the published releases:
#
#   gilfoyle-$VERSION-aarch64-apple-darwin.tar.gz
#   gilfoyle-$VERSION-aarch64-apple-darwin.tar.gz.sha256
#   gilfoyle-$VERSION-x86_64-apple-darwin.tar.gz
#   gilfoyle-$VERSION-x86_64-apple-darwin.tar.gz.sha256
#
# The client is the same `gilfoyle` binary the node runs (`gilfoyle client --config ...`).
# A node is Linux-only (systemd, PackageKit and Docker are reached over D-Bus and a socket),
# so the macOS artifact is the client half of the pair and ships without the site.
#
# No Apple SDK is needed. `zig cc` supplies the Mach-O linker and libSystem stubs, and
# `chrono` is built without its `clock` feature, so the link never asks for the
# CoreFoundation framework. Install the toolchain with `make client-macos-setup`.
set -euo pipefail
cd "$(dirname "$0")/.."

VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
TARGETS="${TARGETS:-aarch64-apple-darwin x86_64-apple-darwin}"

command -v cargo-zigbuild >/dev/null 2>&1 || {
  echo "!! cargo-zigbuild is required: cargo install cargo-zigbuild" >&2
  exit 1
}
command -v zig >/dev/null 2>&1 || {
  echo "!! zig is required: https://ziglang.org/download/ (e.g. 'brew install zig')" >&2
  exit 1
}

for TARGET in $TARGETS; do
  NAME="gilfoyle-${VERSION}-${TARGET}"
  STAGE="dist/${NAME}"

  echo ">> cross-compiling gilfoyle ${VERSION} for ${TARGET}"
  cargo zigbuild --release -p gilfoyle-server --bin gilfoyle --target "$TARGET"

  rm -rf "$STAGE"
  mkdir -p "$STAGE"
  install -m 0755 "target/${TARGET}/release/gilfoyle" "$STAGE/gilfoyle"
  install -m 0644 README.md "$STAGE/README.md"
  install -m 0644 docs/INSTALL.md "$STAGE/INSTALL.md"

  tar -C dist -czf "dist/${NAME}.tar.gz" "$NAME"
  sha256sum "dist/${NAME}.tar.gz" > "dist/${NAME}.tar.gz.sha256"
done

echo ">> wrote:"
ls -l dist/*apple-darwin.tar.gz dist/*apple-darwin.tar.gz.sha256
