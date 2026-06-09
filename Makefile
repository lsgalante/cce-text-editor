.PHONY: build install run clean

build:
	cargo build --release

install: build
	mkdir -p ~/.local/bin
	install -m 755 target/release/cce-text-editor ~/.local/bin/cce-text-editor

run:
	cargo run

clean:
	cargo clean
