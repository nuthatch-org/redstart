# The eject path

Redstart's entire bet is that its generated AssemblyScript is **faithful**: the
canonical `graph build` toolchain compiles it unmodified, and it produces the
*same store* a careful human would.

`redstart build` emits exactly the files a hand-written subgraph has:

```
build/
├── schema.graphql
├── subgraph.yaml
├── abis/…
└── src/mappings.ts
```

These are readable, idiomatic AssemblyScript and GraphQL — not an opaque
intermediate format. You can run `graph codegen` and `graph build` on the output
directly, and `redstart deploy` does exactly that under the hood:

```
redstart build → graph codegen → graph build → graph deploy
```

## Why this matters

The eject path defuses the bus-factor objection to betting production
infrastructure on a young language: **if you ever abandon Redstart, you keep the
generated code, and it keeps working** with the standard toolchain. The cost of
walking away is zero beyond losing the single-source-of-truth convenience.

## Conformance

The claim is continuously verified. The [`conformance/`][conf] harness has three
tiers:

1. **`build`** — proves the eject path: `graph codegen` + `graph build` accept the
   generated subgraph unmodified (needs only Node). This runs in CI for every
   example on every push.
2. **`deploy`** — deploys our subgraph *and* an independently hand-written
   reference to a local graph-node.
3. **`diff`** — the kill/pivot gate: a field-level store-diff of the two at a
   fixed block. Our lowered AssemblyScript must produce a store *identical* to
   idiomatic hand-written AssemblyScript.

[conf]: https://github.com/nuthatch-org/redstart/tree/main/conformance
