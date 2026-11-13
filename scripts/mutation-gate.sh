#!/usr/bin/env bash
# Enforce the 100% mutation-score policy.
#
# cargo-mutants exits with code 2 when any mutant survives, so a clean exit
# is exactly the "all mutants caught" condition we require. Any other nonzero
# code is a tooling error and must fail the gate too.
#
# By default the run is `--in-place`: it reuses the existing target/ directory,
# which keeps disk usage low. `--in-place` also disables parallelism, so a full
# run is serial and slow — fine locally, where the whole gate must run before a
# push, but not in CI.
#
# Set MUTANTS_IN_PLACE=0 to drop it. CI does that so a run can pass `--shard
# k/N` and `--jobs` and fan out; each shard then gets its own copy of the tree,
# which costs disk but keeps the jobs independent.
set -euo pipefail
cd "$(dirname "$0")/.."

if ! command -v cargo-mutants >/dev/null 2>&1 && [ -d "$HOME/.cargo/bin" ]; then
  export PATH="$HOME/.cargo/bin:$PATH"
fi

args=(--workspace)
if [ "${MUTANTS_IN_PLACE:-1}" != "0" ]; then
  args+=(--in-place)
fi

echo ">> cargo-mutants (100% mutation score required)"
if ! cargo mutants "${args[@]}" "$@"; then
  echo "!! mutation gate failed: surviving mutants or tooling error" >&2
  exit 1
fi
echo ">> mutation gate passed: every viable mutant was caught"
