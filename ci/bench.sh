#!/bin/sh

main() {
	set -ex
	git config --global --add safe.directory "$PWD"
	./scripts/bench.sh polkavm
}

main
