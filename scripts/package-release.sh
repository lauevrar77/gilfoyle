#!/usr/bin/env bash
# Build the release binary and package it as a tarball plus a checksum, in the
# layout of the published releases:
#
#   gilfoyle-$VERSION-x86_64-unknown-linux-gnu.tar.gz
#   gilfoyle-$VERSION-x86_64-unknown-linux-gnu.tar.gz.sha256
#
# The tarball holds the binary, the systemd unit, the example polkit rules and
# the docs. Run from the repository root; the artifacts land in dist/.
set -euo pipefail
cd "$(dirname "$0")/.."

VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
TARGET="${TARGET:-x86_64-unknown-linux-gnu}"
NAME="gilfoyle-${VERSION}-${TARGET}"
STAGE="dist/${NAME}"

echo ">> building gilfoyle ${VERSION} (${TARGET})"
cargo build --release -p gilfoyle-server --bin gilfoyle

rm -rf "$STAGE"
mkdir -p "$STAGE"
install -m 0755 target/release/gilfoyle "$STAGE/gilfoyle"
install -m 0644 packaging/gilfoyle.service "$STAGE/gilfoyle.service"
install -m 0644 packaging/49-gilfoyle-units.rules.example "$STAGE/49-gilfoyle-units.rules.example"
install -m 0644 packaging/49-gilfoyle-packagekit.rules.example "$STAGE/49-gilfoyle-packagekit.rules.example"
install -m 0644 README.md "$STAGE/README.md"
install -m 0644 docs/INSTALL.md "$STAGE/INSTALL.md"

tar -C dist -czf "dist/${NAME}.tar.gz" "$NAME"
sha256sum "dist/${NAME}.tar.gz" > "dist/${NAME}.tar.gz.sha256"

echo ">> wrote:"
ls -l "dist/${NAME}.tar.gz" "dist/${NAME}.tar.gz.sha256"
