#!/usr/bin/env bash
# Build the release binary and package it as a tarball plus a checksum, in the
# layout of the published releases:
#
#   gilfoyle-$VERSION-x86_64-unknown-linux-gnu.tar.gz
#   gilfoyle-$VERSION-x86_64-unknown-linux-gnu.tar.gz.sha256
#
# The tarball holds the binary, the systemd unit, the example polkit rules,
# the docs, and the compiled website. A release always ships the site.
set -euo pipefail
cd "$(dirname "$0")/.."

VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
TARGET="${TARGET:-x86_64-unknown-linux-gnu}"
NAME="gilfoyle-${VERSION}-${TARGET}"
STAGE="dist/${NAME}"

echo ">> building gilfoyle ${VERSION} (${TARGET})"
cargo build --release -p gilfoyle-server --bin gilfoyle

echo ">> building the website into website/dist/"
command -v npm >/dev/null 2>&1 || {
  echo "!! npm is required to build the website for a release" >&2
  exit 1
}
( cd website && npm ci && npm run build )
[ -f website/dist/index.html ] || {
  echo "!! the website build produced no website/dist/index.html" >&2
  exit 1
}

rm -rf "$STAGE"
mkdir -p "$STAGE"
install -m 0755 target/release/gilfoyle "$STAGE/gilfoyle"
install -m 0644 packaging/gilfoyle.service "$STAGE/gilfoyle.service"
install -m 0644 packaging/49-gilfoyle-units.rules.example "$STAGE/49-gilfoyle-units.rules.example"
install -m 0644 packaging/49-gilfoyle-packagekit.rules.example "$STAGE/49-gilfoyle-packagekit.rules.example"
install -m 0644 README.md "$STAGE/README.md"
install -m 0644 docs/INSTALL.md "$STAGE/INSTALL.md"
cp -a website/dist "$STAGE/website"

tar -C dist -czf "dist/${NAME}.tar.gz" "$NAME"
sha256sum "dist/${NAME}.tar.gz" > "dist/${NAME}.tar.gz.sha256"

echo ">> wrote:"
ls -l "dist/${NAME}.tar.gz" "dist/${NAME}.tar.gz.sha256"
