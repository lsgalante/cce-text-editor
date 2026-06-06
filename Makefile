.PHONY: build install run clean

build:
	cargo build --release

install: build
	mkdir -p ~/.local/bin
	install -m 755 target/release/clear-text-editor ~/.local/bin/clear-text-editor

run:
	cargo run

clean:
	cargo clean
