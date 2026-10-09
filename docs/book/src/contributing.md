# Contributing & RFCs

Redstart is a public good and contributions are welcome.

## Building from source

```sh
git clone https://github.com/nuthatch-org/redstart
cd redstart
cargo build
cargo test
```

The workspace is a conventional compiler pipeline split across crates:
`redstart-parser` → `redstart-loader` → `redstart-checker` → `redstart-codegen`,
with `redstart-test` (native test interpreter), `redstart-lsp` (language server),
and `redstart-cli` (the `redstart` binary) on top.

CI runs `fmt`, `clippy -D warnings`, the test suite, and the eject-path
conformance build for every example on every push.

## Language design: the RFC process

Substantial changes to the language — new syntax, new semantics, changes to the
generated output — go through a lightweight RFC process so the design is recorded
and discussed before it's built. This is also our answer to the bus-factor
question: the language is specified, not just implemented.

RFCs live in [`rfcs/`][rfcs] in the repository. To propose one:

1. Copy [`rfcs/0000-template.md`][tmpl] to `rfcs/0000-my-feature.md`.
2. Fill it in — motivation, design, alternatives, drawbacks.
3. Open a pull request. The number is assigned when it's accepted.

See [`rfcs/README.md`][rfcs] for the full lifecycle, and
[RFC-0001][rfc1] for the foundational design rationale.

[rfcs]: https://github.com/nuthatch-org/redstart/tree/main/rfcs
[tmpl]: https://github.com/nuthatch-org/redstart/blob/main/rfcs/0000-template.md
[rfc1]: https://github.com/nuthatch-org/redstart/blob/main/rfcs/0001-unified-source-of-truth.md
