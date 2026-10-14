#!/bin/sh

profile=release
rust_version=1.91.0

main() {
	set -e
	case "$1" in
	polkavm) polkavm ;;
	native) native ;;
	*)
		cat >&2 <<EOF
usage: $0 COMMAND

$0 polkavm   run benchmarks under PolkaVM
$0 native    run benchmarks natively
EOF
		exit 1
		;;
	esac
}

polkavm() {
	rustup component add rust-src --toolchain "$rust_version"-x86_64-unknown-linux-gnu
	export RUSTUP_TOOLCHAIN="$rust_version"
	printf "Architecture: PolkaVM\n" >&2
	target_json="$(cargo run --quiet -p corevm-codec-bench-run --profile "$profile" -- target-json-path)"
	guest_target_dir="$PWD"/bench/guest/target
	env RUSTC_BOOTSTRAP=1 cargo build \
		--quiet \
		--manifest-path bench/guest/Cargo.toml \
		--release \
		--target-dir "$guest_target_dir" \
		--target "$target_json" \
		-Zbuild-std=core,alloc \
		-Zbuild-std-features=compiler-builtins-mem
	cargo run --quiet -p corevm-codec-bench-run --profile "$profile" -- run \
		"$guest_target_dir"/"$(basename "$target_json" .json)"/release/corevm-codec-bench-guest \
		bench/quake-frames.rgb888
}

native() {
	printf "Architecture: %s\n" "$(arch)" >&2
	printf "\nCoreVM codec\n" >&2
	native_build_and_run --no-default-features --features corevm
	printf "\nQOI + rANS\n" >&2
	native_build_and_run --no-default-features --features qoi
	printf "\nAV1\n" >&2
	native_build_and_run --no-default-features --features av1
}

native_build_and_run() {
	cargo build -p corevm-codec-bench --profile "$profile" --quiet "$@"
	time target/"$profile"/corevm-codec-bench
}

main "$@"
