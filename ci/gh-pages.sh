#!/bin/sh

main() {
    set -ex
    workdir="$(mktemp -d)"
    trap cleanup EXIT
    generate_documentation
    push_to_gh_pages
}

generate_documentation() {
    cd wasm
    rm -rf pkg
	wasm-pack build --release --out-name corevm_codec --no-typescript
    node js/scaffolding.js
    tsc pkg/*.js --declaration --allowJs --emitDeclarationOnly --outDir pkg
    typedoc --out "$workdir"/docs
    cd ..
}

push_to_gh_pages() {
    mkdir "$workdir"/gh-pages
    git clone https://"$GITHUB_ACTOR":"$GITHUB_TOKEN"@github.com/"$GITHUB_REPOSITORY" "$workdir"/gh-pages
    git -C "$workdir"/gh-pages checkout gh-pages
    rsync -av --exclude .git --delete "$workdir"/docs/ "$workdir"/gh-pages/
    cd "$workdir"/gh-pages
    git add --all
    git config --global user.name "$GITHUB_ACTOR"
    git config --global user.email "$GITHUB_ACTOR@users.noreply.github.com"
    git commit -m "Update" || true
    git push || true
}

cleanup() {
    rm -rf "$workdir"
}

main
