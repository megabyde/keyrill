COLOR_CYAN := \033[36m
COLOR_RESET := \033[0m

# Ad-hoc by default; a fixed self-signed certificate keeps macOS permissions across updates:
# make install SIGN_IDENTITY="My Code Signing Certificate"
SIGN_IDENTITY ?= -
BUNDLE_DIR := target/alpha.noindex
APP := $(BUNDLE_DIR)/KeyrillAlpha.app
INPUT_METHODS := $(HOME)/Library/Input Methods
NATIVE_SOURCES = $(wildcard native/macos/src/*.h native/macos/src/*.mm native/macos/tests/*.mm \
	native/macos/tools/*.mm)
C_SOURCES := include/keyrill.h crates/keyrill-ffi/tests/abi_smoke.c
VERIFY_MODES := sanitize browser-fixture markdown site audit coverage evaluate fuzz miri

.DEFAULT_GOAL := help

.PHONY: help
help: ## Show this help message
	@awk 'BEGIN {FS = ":.*##"; printf "Usage: make [options] $(COLOR_CYAN)[target] ...$(COLOR_RESET)\n\n"} \
	/^[a-zA-Z_-]+:.*##/ {printf "  $(COLOR_CYAN)%-16s$(COLOR_RESET) %s\n", $$1, $$2}' \
	$(MAKEFILE_LIST)

.PHONY: check
check: ## Run the default verification (Rust, C ABI smoke test, native tests on macOS)
	./tools/verify

# These targets carry only their help text; the $(VERIFY_MODES) rule below runs each one as the
# ./tools/verify mode of the same name
.PHONY: sanitize
sanitize: ## Run the native tests under ASan/UBSan

.PHONY: browser-fixture
browser-fixture: ## Run the browser fixture checks (Node 18+)

.PHONY: markdown
markdown: ## Check Markdown formatting and lint it (uv)

.PHONY: site
site: ## Build the documentation site into target/site/book (mdBook)

.PHONY: audit
audit: ## Check advisories, sources, licenses and bans (cargo-deny)

.PHONY: coverage
coverage: ## Measure Rust line and region coverage with the 95% floor

.PHONY: evaluate
evaluate: ## Evaluate the model on the dev and test splits

.PHONY: fuzz
fuzz: ## Run the bounded fuzz targets

.PHONY: miri
miri: ## Run the FFI tests under Miri

$(VERIFY_MODES):
	./tools/verify $@

.PHONY: format
format: ## Format Rust, C, Objective-C++ and Markdown sources in place
	cargo fmt --all
	cargo fmt --manifest-path fuzz/Cargo.toml
	clang-format -i $(C_SOURCES) $(NATIVE_SOURCES)
	./tools/verify markdown fix

.PHONY: bundle
bundle: ## Build KeyrillAlpha.app and sign it with SIGN_IDENTITY
	KEYRILL_NATIVE_OUT="$(CURDIR)/$(BUNDLE_DIR)" native/macos/build.sh bundle
	codesign --force --sign "$(SIGN_IDENTITY)" --timestamp=none $(APP)
	codesign --verify --strict $(APP)

.PHONY: status
status: ## Show the current and Keyrill input sources without changing them
	native/macos/build.sh status

# The server is quit first: copying over the bundle of a running server can get it killed for
# an invalid code signature
.PHONY: install
install: bundle ## Replace the installed bundle; select stock U.S. first
	-pkill -x KeyrillAlpha
	rm -rf "$(INPUT_METHODS)/KeyrillAlpha.app"
	mkdir -p "$(INPUT_METHODS)"
	cp -R $(APP) "$(INPUT_METHODS)/"
	@echo "Installed; select a Keyrill mode to start the new server"

.PHONY: clean
clean: ## Remove build artifacts
	cargo clean
	cargo clean --manifest-path fuzz/Cargo.toml
