# Installation

Redstart ships pre-built binaries for macOS (arm64/x86_64) and Linux
(x86_64/arm64) — no Rust toolchain required.

## Quick install

```sh
curl -fsSL https://raw.githubusercontent.com/nuthatch-org/redstart/main/scripts/install.sh | sh
```

This downloads the binary for your platform from the latest
[release](https://github.com/nuthatch-org/redstart/releases), verifies its
sha256, and installs it to `~/.local/bin` (override with `REDSTART_BIN_DIR`).

## Homebrew

```sh
brew install nuthatch-org/tap/redstart
```

## Cargo

If you'd rather build from source (needs a recent stable Rust):

```sh
cargo install --git https://github.com/nuthatch-org/redstart redstart-cli
```

## Verify

```sh
redstart --version
```

## What you'll also want

To build and deploy a subgraph, the canonical Graph toolchain (`graph-cli`) is
invoked through `redstart deploy`; it runs via `npx`, so you only need Node 18+
on your PATH. Redstart never reimplements `graph build` — it generates the input
to it.
