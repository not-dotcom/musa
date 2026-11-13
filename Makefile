SHELL := /bin/bash
PREFIX := $(CURDIR)/.deps/sysroot/usr
PKG_ENV := PKG_CONFIG_PATH=$(PREFIX)/lib64/pkgconfig$${PKG_CONFIG_PATH:+:$$PKG_CONFIG_PATH}

.PHONY: setup build test fmt clippy run measure clean

setup:
	@./scripts/setup-libadwaita.sh

build:
	@./scripts/build.sh

test:
	@$(PKG_ENV) cargo test --workspace

fmt:
	@cargo fmt --all -- --check

clippy:
	@if command -v cargo-clippy >/dev/null 2>&1; then \
		$(PKG_ENV) cargo clippy --workspace --all-targets -- -D warnings; \
	else \
		echo "cargo-clippy no está instalado; usando cargo check con -D warnings"; \
		$(PKG_ENV) RUSTFLAGS="-D warnings" cargo check --workspace --all-targets; \
	fi

run:
	@./scripts/run.sh

measure:
	@./scripts/measure.sh

clean:
	@cargo clean
