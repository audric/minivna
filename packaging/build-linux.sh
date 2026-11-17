#!/bin/sh
# Builds the Linux release binary and wraps it in an AppImage.
# Needs: cargo, appimagetool (https://github.com/AppImage/appimagetool) on the PATH.
set -eu
cd "$(dirname "$0")/.."
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
arch=$(uname -m)

cargo build --release

if ! command -v appimagetool >/dev/null; then
    echo "appimagetool is not installed: the binary is at target/release/minivna, no AppImage was made." >&2
    exit 1
fi

appdir=target/appimage/minivna.AppDir
rm -rf "$appdir"
mkdir -p "$appdir/usr/bin"
cp target/release/minivna "$appdir/usr/bin/"
cp packaging/minivna.desktop packaging/minivna.png "$appdir/"
cat > "$appdir/AppRun" <<'RUN'
#!/bin/sh
exec "$(dirname "$(readlink -f "$0")")/usr/bin/minivna" "$@"
RUN
chmod +x "$appdir/AppRun"

mkdir -p dist
ARCH=$arch appimagetool "$appdir" "dist/minivna-$version-$arch.AppImage"
echo "dist/minivna-$version-$arch.AppImage"
