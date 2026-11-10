CARGO ?= cargo
FUZZ_RUNS ?= 20000

.PHONY: help build run fmt fmt-check lint test check mutants fuzz package-release install-hooks website-install website-dev website-build website-preview

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
	@echo "  package-release Build + tarball the release artifacts into dist/"
	@echo "  install-hooks Install the git pre-commit hook"
	@echo "  website-install Install the website dependencies (npm ci in website/)"
	@echo "  website-dev   Run the website dev server"
	@echo "  website-build Build the static website into website/dist/"
	@echo "  website-preview Preview the built website"

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
	$(CARGO) fuzz run package_id -- -runs=$(FUZZ_RUNS)
	$(CARGO) fuzz run container_ref -- -runs=$(FUZZ_RUNS)
	$(CARGO) fuzz run container_allowlist -- -runs=$(FUZZ_RUNS)

package-release:
	./scripts/package-release.sh

check: fmt-check lint test mutants

install-hooks:
	./scripts/install-hooks.sh

website-install:
	cd website && npm ci

website-dev:
	cd website && npm run dev

website-build:
	cd website && npm run build

website-preview:
	cd website && npm run preview
