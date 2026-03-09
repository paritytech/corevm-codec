#!/bin/sh

main() {
	set -ex
	git config --global --add safe.directory "$PWD"
	cargo fmt --all --check
	cargo clippy --workspace --all-features --all-targets --quiet -- -Dwarnings
	cargo test --workspace
	wasm_build
	wasm_test
	./scripts/bench.sh native
}

wasm_build() {
	cd wasm
	rm -rf pkg
	wasm-pack build --release --out-name corevm_codec
	node js/scaffolding.js
	cd ..
}

wasm_test() {
	cargo run -p corevm-codec-tool -- \
		-f rgb888 -F corevm --width 320 --height 200 -q 4 \
		<bench/quake-frames.rgb888 >wasm/quake-frames.corevm
	cd wasm
	node --experimental-wasm-modules tests.js
	cd ..
}

main
