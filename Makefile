.PHONY: build install test

BIN_DIR := $(HOME)/.local/bin

build:
	cargo build --release -p illium

install: build
	install -m 755 target/release/illium $(BIN_DIR)/illium

test:
	cargo test --workspace
