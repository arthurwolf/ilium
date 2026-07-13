.PHONY: build install test

BIN_DIR := $(HOME)/.local/bin

build:
	cargo build --release -p ilium -p ilium-server

install: build
	install -m 755 target/release/ilium $(BIN_DIR)/ilium
	install -m 755 target/release/ilium-server $(BIN_DIR)/ilium-server

test:
	cargo test --workspace
