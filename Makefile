CARGO ?= cargo
FUZZ_RUNS ?= 20000

.PHONY: help build run fmt fmt-check lint test check mutants fuzz install-hooks

help:
	@echo "Gilfoyle — available targets:"
	@echo "  build         Build the workspace (debug)"
	@echo "  run           Run the server (reads config from --config, default /etc/gilfoyle/config.toml)"
	@echo "  fmt           Format all crates"
	@echo "  fmt-check     Check formatting (no writes)"
	@echo "  lint          Run clippy with -D warnings"
	@echo "  test          Run the full test suite"
	@echo "  mutants       Run cargo-mutants and require a 100% score"
	@echo "  fuzz          Run the fuzz targets (nightly, ASan)"
	@echo "  check         fmt-check + lint + test + mutants"
	@echo "  install-hooks Install the git pre-commit hook"

build:
	$(CARGO) build --workspace

run:
	$(CARGO) run -p gilfoyle-server --bin gilfoyle --

fmt:
	$(CARGO) fmt --all

fmt-check:
	$(CARGO) fmt --all -- --check

lint:
	$(CARGO) clippy --workspace --all-targets -- -D warnings

test:
	$(CARGO) test --workspace

mutants:
	./scripts/mutation-gate.sh

fuzz:
	$(CARGO) fuzz run uptime_parse -- -runs=$(FUZZ_RUNS)
	$(CARGO) fuzz run authorized_key -- -runs=$(FUZZ_RUNS)
	$(CARGO) fuzz run config_toml -- -runs=$(FUZZ_RUNS)
	$(CARGO) fuzz run client_config -- -runs=$(FUZZ_RUNS)
	$(CARGO) fuzz run unit_name -- -runs=$(FUZZ_RUNS)

check: fmt-check lint test mutants

install-hooks:
	./scripts/install-hooks.sh
