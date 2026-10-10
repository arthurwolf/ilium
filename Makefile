.PHONY: build install test

CARGO_HOME ?= $(HOME)/.cargo
BIN_DIR ?= $(CARGO_HOME)/bin

build:
	cargo build --release -p ilium -p ilium-server -p ilium-animation-js --bin ilium --bin ilium-server --bin ilium-animation-helper

# Binaries are installed only from a verified ni-build receipt:
#   make install RECEIPT=<job-id>
# (the job id is printed by the remote `make build`). The script checks the
# receipt succeeded and every artifact hash, then installs atomically and
# writes <bin>.build.json beside each binary.
install:
	@test -n "$(RECEIPT)" || { echo "make install needs RECEIPT=<ni-build job id>" >&2; exit 2; }
	python3 tools/install-from-receipt.py --receipt $(RECEIPT) --bin-dir $(BIN_DIR)
	install -m 644 ilium-animation-js/assets/packages/beach-1.0.0.iliumanim $(BIN_DIR)/beach-1.0.0.iliumanim
	install -m 644 ilium-animation-js/assets/packages/carpet-1.0.0.iliumanim $(BIN_DIR)/carpet-1.0.0.iliumanim

test:
	cargo test --workspace
