#!/bin/sh
# Install the latest gls release (Linux and macOS).
#   curl -fsSL https://raw.githubusercontent.com/equinox79/gls/main/install.sh | sh
# Options (environment variables):
#   GLS_VERSION      version to install, for example 0.2.0 (default: the latest release)
#   GLS_INSTALL_DIR  where to put gls (default: ~/.local/bin)
#   GLS_NO_MAN       set to 1 to skip installing the manual page
set -eu

REPO="equinox79/gls"
DIR="${GLS_INSTALL_DIR:-$HOME/.local/bin}"

say() { printf '%s\n' "$*"; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }
need() { command -v "$1" >/dev/null 2>&1 || die "$1 is required"; }

need curl
need tar

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64)                TARGET="x86_64-unknown-linux-gnu" ;;
  Linux-aarch64|Linux-arm64)   TARGET="aarch64-unknown-linux-gnu" ;;
  Darwin-arm64)                TARGET="aarch64-apple-darwin" ;;
  Darwin-x86_64)               TARGET="x86_64-apple-darwin" ;;
  *) die "no prebuilt binary for $(uname -s) $(uname -m). Build from source: https://github.com/$REPO#option-2-build-from-source" ;;
esac

VERSION="${GLS_VERSION:-}"
VERSION="${VERSION#v}"
if [ -z "$VERSION" ]; then
  # /releases/latest redirects to /releases/tag/vX.Y.Z (no API, no rate limit)
  url=$(curl -fsSLI -o /dev/null -w '%{url_effective}' "https://github.com/$REPO/releases/latest") \
    || die "could not look up the latest release"
  VERSION="${url##*/v}"
fi
[ -n "$VERSION" ] || die "could not work out the version"

NAME="gls-v$VERSION-$TARGET"
BASE="https://github.com/$REPO/releases/download/v$VERSION"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

say "Installing gls $VERSION ($TARGET) to $DIR"
curl -fsSL -o "$TMP/$NAME.tar.gz" "$BASE/$NAME.tar.gz" || die "download failed: $BASE/$NAME.tar.gz"
curl -fsSL -o "$TMP/SHA256SUMS" "$BASE/SHA256SUMS" || die "download failed: $BASE/SHA256SUMS"

want=$(grep " $NAME.tar.gz\$" "$TMP/SHA256SUMS" | awk '{print $1}')
[ -n "$want" ] || die "$NAME.tar.gz is not in SHA256SUMS"
if command -v sha256sum >/dev/null 2>&1; then
  got=$(sha256sum "$TMP/$NAME.tar.gz" | awk '{print $1}')
elif command -v shasum >/dev/null 2>&1; then
  got=$(shasum -a 256 "$TMP/$NAME.tar.gz" | awk '{print $1}')
else
  die "sha256sum or shasum is required to check the download"
fi
[ "$want" = "$got" ] || die "checksum mismatch (expected $want, got $got)"

tar xzf "$TMP/$NAME.tar.gz" -C "$TMP"
mkdir -p "$DIR"
cp "$TMP/$NAME/gls" "$DIR/gls"
chmod +x "$DIR/gls"

say "Installed: $DIR/gls"
"$DIR/gls" --version || true

# The manual page (gls --man was added after 0.2.1; older versions are skipped). It is written in
# your language when gls has it. Skip it with GLS_NO_MAN=1.
if [ -z "${GLS_NO_MAN:-}" ]; then
  MANDIR="${XDG_DATA_HOME:-$HOME/.local/share}/man/man1"
  if "$DIR/gls" --man > "$TMP/gls.1" 2>/dev/null && [ -s "$TMP/gls.1" ]; then
    mkdir -p "$MANDIR" && cp "$TMP/gls.1" "$MANDIR/gls.1" && say "Manual page: $MANDIR/gls.1  (try: man gls)"
  fi
fi
case ":$PATH:" in
  *":$DIR:"*) ;;
  *) say "Note: $DIR is not on your PATH. See \"Put gls on your PATH\" in the README:"
     say "  https://github.com/$REPO#install" ;;
esac
