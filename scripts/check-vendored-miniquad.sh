#!/bin/sh
# Checks that vendor/miniquad-0.4.11 is exactly miniquad 0.4.11 as published
# on crates.io plus vendor/patches/*.patch, and nothing else.
# Needs network access to static.crates.io. See docs/UPSTREAM_FIXES.md.
set -eu
cd "$(dirname "$0")/.."
version=0.4.11
sha256=dcbaea978d741f3b60ced666b06a2a1e4a2bc81cb8b9dd29f4a40965e1f59e65
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
curl -fsSL -o "$work/miniquad.crate" \
    "https://static.crates.io/crates/miniquad/miniquad-$version.crate"
echo "$sha256  $work/miniquad.crate" | sha256sum -c --quiet -
tar xzf "$work/miniquad.crate" -C "$work"
for p in vendor/patches/miniquad-$version-*.patch; do
    patch -s -d "$work/miniquad-$version" -p1 < "$p"
done
diff -r "$work/miniquad-$version" "vendor/miniquad-$version"
echo "vendor/miniquad-$version = crates.io miniquad $version + vendor/patches"
