#!/bin/sh
# Cross-builds the Windows program on Linux and zips it with the README.
# Needs: rustup, zip, and the mingw-w64 system package (x86_64-w64-mingw32-gcc).
set -eu
cd "$(dirname "$0")/.."
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
target=x86_64-pc-windows-gnu

if ! command -v x86_64-w64-mingw32-gcc >/dev/null; then
    echo "x86_64-w64-mingw32-gcc is not installed (Debian/Ubuntu: sudo apt install mingw-w64)." >&2
    exit 1
fi

rustup target add "$target"
cargo build --release --target "$target"

mkdir -p dist
rm -f "dist/minivna-$version-windows-x86_64.zip"
zip -j "dist/minivna-$version-windows-x86_64.zip" "target/$target/release/minivna.exe" README.md
echo "dist/minivna-$version-windows-x86_64.zip"
