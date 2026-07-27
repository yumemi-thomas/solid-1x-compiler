RUST_TOOLCHAIN ?= 1.97
COMPILER_MANIFEST := packages/compiler/Cargo.toml

.PHONY: build test test-rust parity verify clean

build:
	pnpm run build

# Rust classification tests, a debug addon build, then the full Jest suite.
test:
	pnpm run test

test-rust:
	cargo +$(RUST_TOOLCHAIN) test --manifest-path $(COMPILER_MANIFEST) --features native-facts

parity:
	pnpm run parity

verify:
	cargo +$(RUST_TOOLCHAIN) fmt --manifest-path $(COMPILER_MANIFEST) -- --check
	cargo +$(RUST_TOOLCHAIN) clippy --manifest-path $(COMPILER_MANIFEST) --features native-facts --all-targets
	$(MAKE) test
	$(MAKE) parity

clean:
	rm -rf packages/compiler/target node_modules packages/*/node_modules
