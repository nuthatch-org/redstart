# Introduction

**Redstart is a multi-file language for authoring [The Graph](https://thegraph.com) subgraphs.**

A subgraph today is three loosely-coupled artifacts — `schema.graphql`,
`subgraph.yaml`, and AssemblyScript mappings — stitched together by
stringly-typed names and a manual `graph codegen` step. Drift between them is the
dominant source of *"it compiled but failed at runtime, three hours into a
sync."*

Redstart unifies all three into **one language**, split across as many `.red`
modules as you like (`mod`/`use`, just like Rust). It type-checks them against
each other and transpiles to readable AssemblyScript that the canonical
`graph build` toolchain compiles **unmodified**.

```redstart
abi ERC20 from "./abis/ERC20.json"

entity Account {
  id: Id<Bytes>
  balance: BigInt
  label: Option<String>          // nullability is always explicit — there is no `null`
}

source Token {
  abi: ERC20
  network: mainnet
  address: 0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48
  startBlock: 6082465
}

handler on Token.Transfer(event) {
  let receiver = Account.loadOrCreate(event.params.to, { balance: BigInt.zero })
  receiver.balance = receiver.balance + event.params.value
  // auto-saved at handler end — forgetting `.save()` can't happen
}
```

`redstart build` turns that into `schema.graphql` + `subgraph.yaml` +
`mappings.ts`. The event signature in the manifest is derived from the ABI *by
reference* — rename the event and it's a **compile** error, not a runtime one.

> **Try it now** — the [playground](https://nuthatch-org.github.io/redstart/playground/)
> runs the real compiler in your browser: write `.red`, watch the generated
> AssemblyScript, schema, and manifest update as you type.

## Why a language?

The killer feature is **unification, not syntax**. A single source of truth makes
manifest/schema/handler drift impossible. The whole class of AssemblyScript
footguns — nullable-arithmetic miscompiles, `==`/`===` inversion, reverted-call
aborts, array prefill, forgotten `.save()` — becomes *unrepresentable by
construction*.

The **eject path** (see [How it works](./eject-path.md)) means abandoning
Redstart costs nothing but the generated code, which keeps working. Redstart does
**not** make indexing faster; it makes *staying on The Graph's decentralized
network pleasant*. It is scoped as a Graph-Foundation-grant public good in the
lineage of Matchstick, not a venture bet.
