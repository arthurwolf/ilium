.PHONY: build install test

CARGO_HOME ?= $(HOME)/.cargo
BIN_DIR ?= $(CARGO_HOME)/bin

build:
	cargo build --release -p ilium -p ilium-server -p ilium-animation-js --bin ilium --bin ilium-server --bin ilium-animation-helper

install: build
	install -d -m 755 $(BIN_DIR)
	install -m 755 target/release/ilium $(BIN_DIR)/ilium
	install -m 755 target/release/ilium-server $(BIN_DIR)/ilium-server
	install -m 755 target/release/ilium-animation-helper $(BIN_DIR)/ilium-animation-helper
	install -m 644 ilium-animation-js/assets/packages/beach-1.0.0.iliumanim $(BIN_DIR)/beach-1.0.0.iliumanim
	install -m 644 ilium-animation-js/assets/packages/carpet-1.0.0.iliumanim $(BIN_DIR)/carpet-1.0.0.iliumanim

test:
	cargo test --workspace
