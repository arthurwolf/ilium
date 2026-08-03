.PHONY: build install test

CARGO_HOME ?= $(HOME)/.cargo
BIN_DIR ?= $(CARGO_HOME)/bin

build:
	cargo build --release -p ilium -p ilium-server

install: build
	install -d -m 755 $(BIN_DIR)
	install -m 755 target/release/ilium $(BIN_DIR)/ilium
	install -m 755 target/release/ilium-server $(BIN_DIR)/ilium-server

test:
	cargo test --workspace
