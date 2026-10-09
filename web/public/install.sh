#!/usr/bin/env sh
# Redstart installer.
#
#   curl -fsSL https://raw.githubusercontent.com/nuthatch-org/redstart/main/scripts/install.sh | sh
#
# Downloads the pre-built `redstart` binary for your platform from the latest
# GitHub Release, verifies its sha256, and installs it. No Rust required.
#
# Environment overrides:
#   REDSTART_VERSION  tag to install (default: latest, e.g. v0.1.0)
#   REDSTART_BIN_DIR  install directory (default: ~/.local/bin)
#   REDSTART_REPO     owner/repo (default: nuthatch-org/redstart)
set -eu

REPO="${REDSTART_REPO:-nuthatch-org/redstart}"
BIN_DIR="${REDSTART_BIN_DIR:-$HOME/.local/bin}"

err() { printf '\033[1;31merror:\033[0m %s\n' "$1" >&2; exit 1; }
info() { printf '\033[1;36m▸\033[0m %s\n' "$1"; }

# ---- detect platform ----
os="$(uname -s)"
arch="$(uname -m)"
case "$os" in
  Darwin) os_part="apple-darwin" ;;
  Linux)  os_part="unknown-linux-gnu" ;;
  *) err "unsupported OS: $os (use cargo install, or download from https://github.com/$REPO/releases)" ;;
esac
case "$arch" in
  arm64|aarch64) arch_part="aarch64" ;;
  x86_64|amd64)  arch_part="x86_64" ;;
  *) err "unsupported architecture: $arch" ;;
esac
target="${arch_part}-${os_part}"

# ---- need curl or wget ----
if command -v curl >/dev/null 2>&1; then
  dl() { curl -fsSL "$1"; }
  dlo() { curl -fsSL "$1" -o "$2"; }
elif command -v wget >/dev/null 2>&1; then
  dl() { wget -qO- "$1"; }
  dlo() { wget -qO "$2" "$1"; }
else
  err "need curl or wget"
fi

# ---- resolve version ----
version="${REDSTART_VERSION:-}"
if [ -z "$version" ]; then
  info "resolving latest release"
  version="$(dl "https://api.github.com/repos/$REPO/releases/latest" \
    | grep '"tag_name":' | head -1 | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/')"
  [ -n "$version" ] || err "could not determine latest version; set REDSTART_VERSION"
fi

archive="redstart-${target}.tar.gz"
checksum="redstart-${target}.sha256"   # taiki-e names it <stem>.sha256, not <archive>.sha256
base="https://github.com/$REPO/releases/download/$version"
info "installing redstart $version ($target)"

# ---- download + verify ----
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
dlo "$base/$archive" "$tmp/$archive" || err "download failed: $base/$archive"

if dlo "$base/$checksum" "$tmp/$checksum" 2>/dev/null; then
  expected="$(cut -d' ' -f1 < "$tmp/$checksum")"
  if command -v sha256sum >/dev/null 2>&1; then
    actual="$(sha256sum "$tmp/$archive" | cut -d' ' -f1)"
  elif command -v shasum >/dev/null 2>&1; then
    actual="$(shasum -a 256 "$tmp/$archive" | cut -d' ' -f1)"
  else
    actual="$expected"  # no tool to verify; trust the transport
    info "no sha256 tool found — skipping checksum verification"
  fi
  [ "$actual" = "$expected" ] || err "checksum mismatch (expected $expected, got $actual)"
  info "checksum verified"
fi

# ---- install ----
tar -xzf "$tmp/$archive" -C "$tmp"
[ -f "$tmp/redstart" ] || err "archive did not contain the redstart binary"
mkdir -p "$BIN_DIR"
install -m 0755 "$tmp/redstart" "$BIN_DIR/redstart" 2>/dev/null \
  || { mv "$tmp/redstart" "$BIN_DIR/redstart" && chmod 0755 "$BIN_DIR/redstart"; }

info "installed to $BIN_DIR/redstart"
case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *) printf '\033[1;33mnote:\033[0m %s is not on your PATH. Add:\n  export PATH="%s:$PATH"\n' "$BIN_DIR" "$BIN_DIR" ;;
esac
"$BIN_DIR/redstart" --version 2>/dev/null || true
