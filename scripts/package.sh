#!/usr/bin/env bash
# Package a release build into an archive.
#
#   scripts/package.sh <target> <tar.gz|zip> <name> [outdir]
#
# Expects `cargo build --release --target <target>` to have been run. Creates
# <outdir>/<name>.<tar.gz|zip> holding a folder <name>/ with the binary, the READMEs and the
# license. Used by .github/workflows/release.yml; can be run by hand to test it.
set -euo pipefail

target=${1:?target triple, e.g. x86_64-unknown-linux-gnu}
kind=${2:?archive type: tar.gz or zip}
name=${3:?archive name without extension}
out=${4:-dist}

bin=gls
[ "$kind" = zip ] && bin=gls.exe
src="target/$target/release/$bin"
[ -f "$src" ] || { echo "missing $src (run cargo build --release --target $target first)" >&2; exit 1; }

mkdir -p "$out"
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
mkdir "$stage/$name"
cp "$src" README.md README_JA.md LICENSE "$stage/$name/"

case "$kind" in
  tar.gz) tar -C "$stage" -czf "$out/$name.tar.gz" "$name" ;;
  zip)
    if command -v 7z >/dev/null 2>&1; then
      (cd "$stage" && 7z a -tzip -bso0 "$OLDPWD/$out/$name.zip" "$name")
    elif command -v zip >/dev/null 2>&1; then
      (cd "$stage" && zip -qr "$OLDPWD/$out/$name.zip" "$name")
    else
      # Windows PowerShell as the last resort
      powershell -NoProfile -Command "Compress-Archive -Path '$stage/$name' -DestinationPath '$out/$name.zip'"
    fi
    ;;
  *) echo "unknown archive type: $kind" >&2; exit 1 ;;
esac
echo "$out/$name.$kind"
