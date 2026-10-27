#!/usr/bin/env bash
# Enforce the 100% mutation-score policy.
#
# cargo-mutants exits with code 2 when any mutant survives, so a clean exit
# is exactly the "all mutants caught" condition we require. Any other nonzero
# code is a tooling error and must fail the gate too.
#
# --in-place reuses the existing target/ directory, which keeps disk usage
# low. It also disables parallelism, so a full run is serial and slow.
set -euo pipefail
cd "$(dirname "$0")/.."

if ! command -v cargo-mutants >/dev/null 2>&1 && [ -d "$HOME/.cargo/bin" ]; then
  export PATH="$HOME/.cargo/bin:$PATH"
fi

echo ">> cargo-mutants (100% mutation score required)"
if ! cargo mutants --workspace --in-place "$@"; then
  echo "!! mutation gate failed: surviving mutants or tooling error" >&2
  exit 1
fi
echo ">> mutation gate passed: every viable mutant was caught"
