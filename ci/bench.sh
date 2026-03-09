#!/bin/sh

main() {
	set -ex
	workdir="$(mktemp -d)"
	trap cleanup EXIT
	root="$PWD"
	polkaports_install corevm
	corevm_benchmarks
}

corevm_benchmarks() {
	cd "$root"
	git config --global --add safe.directory "$PWD"
    ./scripts/bench.sh riscv
}

polkaports_install() {
	sudo -n apt-get -qq update
	sudo -n apt-get -qq install -y clang-20 lld-20 llvm-20 autotools-dev nasm
	rustup component add rust-src
	git clone --recurse-submodules https://github.com/paritytech/polkaports "$workdir"/polkaports
	cd "$workdir"/polkaports
	env CC=clang-20 \
		LD=clang-20 \
		LLD=lld-20 \
		AR=llvm-ar-20 \
		AS=llvm-as-20 \
		NM=llvm-nm-20 \
		STRIP=llvm-strip-20 \
		OBJCOPY=llvm-objcopy-20 \
		OBJDUMP=llvm-objdump-20 \
		RANLIB=llvm-ranlib-20 \
		./setup.sh
	. ./activate.sh corevm
}

cleanup() {
	rm -rf "$workdir"
}

main
