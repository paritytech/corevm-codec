#!/bin/sh

profile=release
rust_target=riscv64emac-corevm-linux-musl
rust_stack_size=8388608

main() {
	set -e
	case "$1" in
	riscv) riscv ;;
	native) native ;;
	*)
		cat >&2 <<EOF
usage: $0 COMMAND

$0 riscv     run benchmarks in CoreVM
$0 native    run benchmarks natively
EOF
		exit 1
		;;
	esac
}

riscv() {
	printf "Architecture: RISCV\n" >&2
	printf "CoreVM codec\n" >&2
	riscv_build_and_run --no-default-features --features corevm
	printf "QOI + rANS\n" >&2
	riscv_build_and_run --no-default-features --features qoi
	printf "AV1\n" >&2
	riscv_build_and_run --no-default-features --features av1
}

riscv_build_and_run() {
	corevm_build -p corevm-codec-bench --profile "$profile" --quiet "$@"
	cargo run --quiet -p corevm-run --bin corevm-run --profile "$profile" -- \
		--perf \
		target/"$rust_target"/"$profile"/corevm-codec-bench.corevm
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

corevm_build() {
	if test -z "$POLKAPORTS_SYSROOT"; then
		printf "You need to install PolkaPorts from https://github.com/paritytech/polkaports and run \`./activate.sh corevm\` before you can build Rust binaries for CoreVM.\n" >&2
		exit 1
	fi
	profile=debug
	package=
	scan_cargo_args "$@"
	printf "Package: %s\n" "$package" >&2
	printf "Profile: %s\n" "$profile" >&2
	output_file=target/"$rust_target"/"$profile"/"$package".corevm
	oldpwd="$PWD"
	cd "$oldpwd"
	env RUSTC_BOOTSTRAP=1 \
		RUSTUP_TOOLCHAIN=1.90.0 \
		cargo build \
		"$@" \
		--target="$POLKAPORTS_SYSROOT"/"$rust_target".json \
		-Zbuild-std=core,alloc,std,panic_abort \
		-Zbuild-std-features=panic_immediate_abort
	cd "$oldpwd"
	polkatool link --min-stack-size "$rust_stack_size" \
		target/"$rust_target"/"$profile"/"$package" \
		-o "$output_file"
	jam-blob set-meta \
		--name "$package" \
		--version 0.1 \
		--license 'Apache-2.0' \
		--author 'Parity Technologies <admin@parity.io>' \
		"$output_file"
	printf "CoreVM binary: %s\n" "$output_file" >&2
}

scan_cargo_args() {
	while test -n "$1"; do
		case "$1" in
		-p | --package)
			shift
			package="$1"
			;;
		--profile)
			shift
			profile="$1"
			;;
		--release)
			profile=release
			;;
		*) ;;
		esac
		shift
	done
}

main "$@"
