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
	sudo -n apt-get -qq install -y clang-19 lld-19 llvm-19 autotools-dev
	rustup component add rust-src
	git clone --recurse-submodules https://github.com/paritytech/polkaports "$workdir"/polkaports
	cd "$workdir"/polkaports
	env CC=clang-19 \
		LD=clang-19 \
		LLD=lld-19 \
		AR=llvm-ar-19 \
		AS=llvm-as-19 \
		NM=llvm-nm-19 \
		STRIP=llvm-strip-19 \
		OBJCOPY=llvm-objcopy-19 \
		OBJDUMP=llvm-objdump-19 \
		RANLIB=llvm-ranlib-19 \
		./setup.sh
	. ./activate.sh corevm
}

cleanup() {
	rm -rf "$workdir"
}

main
