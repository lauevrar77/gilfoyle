#!/usr/bin/env bash
# Install the git pre-commit hook. Fast checks only; the mutation gate is far
# too slow to run on every commit and lives in `make check` instead.
set -euo pipefail
cd "$(dirname "$0")/.."

mkdir -p .git/hooks
cat > .git/hooks/pre-commit <<'HOOK'
#!/usr/bin/env bash
set -euo pipefail
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
HOOK
chmod +x .git/hooks/pre-commit
echo ">> installed .git/hooks/pre-commit"
