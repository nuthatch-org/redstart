# Redstart

**The most performant and secure language for authoring The Graph subgraphs.**

Unifying schema, manifest, and mappings into one typed source is just the start.
Because Redstart owns the layer that emits the AssemblyScript, it generates code
that's *faster and safer than any human would hand-write* — and proves it indexes
byte-identically against a live graph-node. **If it compiles, it works.**

Today a subgraph is three loosely-coupled artifacts — `schema.graphql`,
`subgraph.yaml`, and AssemblyScript mappings — stitched together by stringly-typed
names and a manual `graph codegen` step. Drift between them is the dominant source
of *"it compiled but failed at runtime, three hours into a sync."*

Redstart unifies all three into one language — split across as many `.red` modules
as you like (`mod`/`use`, just like Rust) — type-checks them against each other,
and transpiles to readable AssemblyScript that the canonical `graph build`
toolchain compiles unmodified. Entities can live in one module and the handlers
that write them in another; the compiler resolves and checks across all of them. The entire class of AssemblyScript footguns —
nullable-arithmetic miscompiles, `==`/`===` inversion, reverted-call aborts,
array prefill, forgotten `.save()` — becomes **unrepresentable by construction**.

```redstart
abi ERC20 from "./abis/ERC20.json"

entity Account {
  id: Id<Bytes>
  balance: BigInt
  label: Option<String>          // nullability is always explicit — there is no `null`
  transfersOut: [Transfer] derived from from
}

entity Transfer immutable {
  id: Id<Bytes>
  from: Account
  to: Account
  value: BigInt
  timestamp: BigInt
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
  // auto-saved at handler end (dirty-tracked) — forgetting `.save()` can't happen
}
```

`redstart build` turns that into `schema.graphql` + `subgraph.yaml` +
`mappings.ts`. The event signature in the manifest
(`Transfer(indexed address,indexed address,uint256)`) is derived from the ABI by
reference — rename the event and it's a *compile* error, not a runtime one.

## Why

The killer feature is **unification, not syntax**. A single source of truth makes
manifest/schema/handler drift impossible. The **eject path** — readable emitted
AssemblyScript the canonical toolchain consumes unmodified — means abandoning
Redstart costs nothing but the generated code, which keeps working. That defuses
the bus-factor objection to betting production infra on one language.

Redstart does **not** make indexing faster; it makes *staying on The Graph's
decentralized network pleasant*. It is scoped as a Graph-Foundation-grant public
good in the lineage of Matchstick, not a venture bet.

## Status

✅ **Stage 0 complete — kill-gate GREEN; now an optimising compiler (v0.10.0).**
The unified language is real and end-to-end: a real subgraph is ported and
deployed live to Subgraph Studio, the canonical `graph build` compiles our output
unmodified, and the field-level store-diff proves byte-identical indexing against a
hand-written reference (the project's #1 risk, retired). The frontier now is
[Lever 2 — the optimising compiler](docs/ROADMAP-2026.md#4-pillar-performance--be-an-optimising-compiler):
inferred `@entity(immutable)` and `prune: auto` ship today; Bytes-id inference,
`@derivedFrom` array rewrites, and auto-`startBlock` are next.

| Component | Crate | State |
|---|---|---|
| Lexer + parser (`logos` + recursive descent, `miette` diagnostics) | `redstart-parser` | ✅ working |
| `redstart.toml` manifest + multi-file module tree (cycle detection) | `redstart-loader` | ✅ working |
| `schema.graphql` + `subgraph.yaml` generation from the unified AST | `redstart-codegen` | ✅ working |
| AssemblyScript mapping lowering — `loadOrCreate`, `BigInt`/`BigDecimal` operators, auto-save dirty-tracking, contract calls (`Result` → `try_*`), `match` | `redstart-codegen` | ✅ vertical slice (ERC-20) |
| Control flow — `if`/`else if`/`else`, `while`, `for` (numeric ranges + list iteration), array literals & indexing, lowered to native AssemblyScript | `redstart-codegen` | ✅ working |
| Helper functions — free `fn` declarations lowered to AssemblyScript, cross-module, with return-typed calls and `return`-flushed auto-saves | `redstart-codegen` | ✅ working |
| Handler kinds — event (`handler on Src.Event`), call (`handler call Src.fn`), and block (`handler block Src [every N\|once]`) → `eventHandlers`/`callHandlers`/`blockHandlers` | `redstart-codegen` | ✅ working |
| Dynamic data sources — `template` blocks + `<Template>.create(addr)` / `.createWithContext(addr, ctx)` and `DataSourceContext`, the factory pattern | `redstart-codegen` | ✅ working |
| File data sources — `template T { kind: file }` + `handler file T(content)` → `kind: file/ipfs` manifest, the off-chain-metadata (IPFS) pattern | `redstart-codegen` | ✅ working |
| graph-ts surface — `log`, `crypto`, `dataSource`, `store`, `json`, `ipfs`, `ethereum` namespaces + fuller `BigInt`/`BigDecimal`/`Bytes`/`Address` statics & methods, with whole-word import inference | `redstart-codegen` | ✅ working |
| Schema breadth — `enum` declarations, `interface` + `entity X implements Y & Z` (with field-completeness checking), `Int8` / `Timestamp` scalars, `@derivedFrom`, `@entity(immutable/timeseries)` | `redstart-codegen` | ✅ working |
| Timeseries & aggregations — `entity Data timeseries { … }` (auto `id`/`timestamp`, implicitly immutable) + `aggregation Stats over Data every [hour, day] { total: BigDecimal = sum(price) }` → `@aggregation`/`@aggregate`, auto-bumps `specVersion` to 1.1.0 | `redstart-codegen` | ✅ working |
| Semantic checker — unknown source/event/type, missing source settings, `derived` back-refs, required-field init, `.value`-without-`match`, arithmetic-on-`Option`, **deref-of-nullable** (`load`/`loadInBlock`/`ipfs.cat` return `Option<T>` — must be `match`ed), assign-to-`derived`, **determinism** (`Date.now`/`Math.random` forbidden — PoI-divergence) | `redstart-checker` | ✅ working |
| Performance & correctness **lints** (warnings) — `eth_call` inside a loop, unfiltered block handler, call handler on a non-tracing network; warning-severity diagnostics that report but don't fail the build | `redstart-checker` | ✅ working |
| Agent-native diagnostics — `redstart check --json` (machine-readable `{code, severity, message, help, line, column}`) and `redstart explain <CODE>` (every code's *why* + fix) | `redstart-cli` | ✅ working |
| ABI normalisation on build — emitted ABIs gain `anonymous` on events, so `graph deploy` accepts them (graph-node requires it; `graph build` doesn't) | `redstart-codegen` | ✅ working |
| `redstart test` — native test interpreter (mock store + mocked calls, no WASM/Docker/Matchstick) | `redstart-test` | ✅ working |
| `redstart fmt` — canonical, comment-preserving formatting (`--check` mode) | `redstart-cli` | ✅ working |
| `redstart dev` — watch loop re-running check → build → test on every change | `redstart-cli` | ✅ working |
| `redstart verify` — build → `graph codegen` → `graph build`: proves the generated AssemblyScript compiles to WASM, not merely that the Redstart is valid | `redstart-cli` | ✅ working |
| `redstart deploy` — build → `graph codegen` → `graph build` → `graph deploy` (Studio or self-hosted), with `--dry-run` | `redstart-cli` | ✅ working |
| Tree-sitter grammar + highlight queries — generated, parses every example with 0 errors | `tree-sitter-redstart` | ✅ working |
| `redstart lsp` — language server: diagnostics, formatting, symbols, hover, go-to-def, completion | `redstart-lsp` | ✅ working |
| VS Code extension (LSP client + TextMate highlighting) | `editors/vscode` | ✅ working |
| Zed extension (tree-sitter highlighting + `redstart lsp`) | `editors/zed` | ✅ working |

The AssemblyScript lowering is the whole bet: the **kill/pivot threshold** is a
field-level store-diff against canonical subgraph deployments. The harness for it
lives in [`conformance/`](conformance/) — `./conformance/run.sh build` proves the
eject path (canonical `graph build` compiles our output unmodified) with only
Node; `run.sh all` deploys our subgraph alongside an idiomatic hand-written
reference and store-diffs them at a fixed block.

> **✅ Kill-gate GREEN — indexing fidelity proven.** `run.sh all` deployed
> [`conformance/fixtures/arb-erc20`](conformance/fixtures/arb-erc20) (the ARB
> token on Arbitrum One) to a live graph-node alongside the independent
> hand-written reference and store-diffed them at block 477,660,492:
> **0 differences** across 10 `Account` + 13 `Transfer` entities. Our lowered
> AssemblyScript indexes *byte-identically* to what a careful human writes — the
> roadmap's #1 risk, retired.

> **✅ Eject path proven — for the whole feature surface.** `graph codegen` +
> `graph build` compile the generated subgraph unmodified into WebAssembly, with
> zero manual edits. This now holds not just for the ERC-20 slice but for
> [`examples/factory`](examples/factory) — a single project exercising **event,
> call, and block handlers** (on a source *and* a template), **dynamic data
> sources** (`createWithContext` + context), **control flow**, and an **enum**.
> Run it yourself: `./conformance/run.sh build PROJECT=examples/factory`.
> (Finding the template-import-path bug this caught is exactly why the gate exists.)

## Install

```sh
# Quick install (macOS + Linux) — downloads the pre-built binary, no Rust required
curl -fsSL https://raw.githubusercontent.com/nuthatch-org/redstart/main/scripts/install.sh | sh

# Homebrew (macOS + Linux)
brew install nuthatch-org/tap/redstart

# Cargo (needs a Rust toolchain)
cargo install --git https://github.com/nuthatch-org/redstart redstart-cli
```

Or grab a pre-built binary for macOS (arm64/x86_64) or Linux (x86_64/arm64)
straight from the [latest release](https://github.com/nuthatch-org/redstart/releases/latest).
Any of these put a `redstart` binary on your `PATH` — then just run `redstart …`.

## Playground

Try Redstart in your browser — write `.red` on the left, watch the generated
`mappings.ts` / `schema.graphql` / `subgraph.yaml` regenerate as you type:
**[nuthatch-org.github.io/redstart/playground](https://nuthatch-org.github.io/redstart/playground/)**.
It runs the real compiler (loader → checker → codegen) compiled to WebAssembly —
no server, no install. Build it locally with `./playground/build.sh`.

## Try it

Once installed, it's just `redstart`:

```sh
redstart new my-subgraph
cd my-subgraph
redstart dev                          # watch loop: check → build → test on save
redstart build                        # emit schema.graphql + subgraph.yaml + mappings.ts
redstart verify                       # …and prove the output compiles to WASM
redstart deploy my-slug --dry-run     # build → graph codegen → graph build (no network)
redstart deploy my-slug               # …and graph deploy to Subgraph Studio
```

**Porting an existing subgraph?** Start with the
[porting guide](docs/book/src/porting.md): the schema and mapping translation
tables, what an event handler can and cannot see, and the pattern that replaces
receipt inspection.

Want to poke at the worked examples? Clone the repo and point `redstart` at them:

```sh
git clone https://github.com/nuthatch-org/redstart && cd redstart

# a real-world subgraph: a faithful port of PaulieB14's Graph Horizon indexer —
# 3 Arbitrum contracts, helpers, timeseries/aggregations. Ejects to WASM
# unmodified; 7 native handler tests. See examples/horizon-indexer/README.md.
redstart test examples/horizon-indexer

redstart check examples/erc20         # erc20, split across two modules
redstart build examples/erc20
redstart test  examples/erc20
redstart fmt --check examples/erc20
```

> **Hacking on Redstart itself?** Skip the install and run it straight from the
> checkout: swap `redstart` for `cargo run -p redstart-cli --`
> (e.g. `cargo run -p redstart-cli -- check examples/erc20`).

## Project layout

A project is a `redstart.toml` plus a tree of `.red` modules. The entry module
pulls in others with `mod`; any module can reference another's declarations.

```
my-subgraph/
  redstart.toml        # [project] name / entry / out_dir
  src/
    main.red           # mod accounts;  +  abi / source / handler
    accounts.red       # entity Account, entity Transfer
    abis/ERC20.json
  build/               # generated: schema.graphql, subgraph.yaml, src/mappings.ts, abis/
```

`mod accounts;` resolves to `accounts.red` (or `accounts/mod.red`), exactly like
Rust. The example's `Token.Transfer` handler in `main.red` loads and writes the
`Account` and `Transfer` entities declared in `accounts.red` — across modules,
type-checked, no drift.

## Testing

`redstart test` runs your `test` blocks **natively** — a tree-walking interpreter
evaluates handler ASTs against an in-memory mock store. No WASM compile, no
downloaded Matchstick binary, no Docker, and — because tests are written in
Redstart, not AssemblyScript — no `matchstick-as`/`graph-ts` version skew. Event
fixtures are synthesised from a record literal; contract reads are mocked inline:

```redstart
test "a transfer debits the sender and credits the receiver" {
  Token.Transfer({ from: 0x01, to: 0x02, value: 100 })
  assertEq(Account.at(0x02).balance, 100)
  assert(Account.at(0x01).balance < 0)
}

test "approval writes the balance read via a contract call" {
  mockCall(ERC20.balanceOf(0x05), 4200)        // mock the eth_call
  Token.Approval({ owner: 0x05, spender: 0x06, value: 1 })
  assertEq(Account.at(0x05).balance, 4200)
}
```

This is the fast inner loop for *handler logic*. Fidelity to the real compiled
WASM is the job of the [conformance gate](conformance/), which store-diffs a real
graph-node deployment against a canonical reference. Two layers, two concerns.

## Architecture

A small, batteries-included, single-binary toolchain (the Gleam/Elm/Prisma
model). Crates are split by compiler phase:

```
redstart-parser   lex → AST  (source of all spans & diagnostics)
redstart-loader   redstart.toml + `mod` resolution → ModuleTree
redstart-checker  ModuleTree → semantic analysis → Checked symbol table (RTy/ABI)
redstart-codegen  ModuleTree + Checked → schema.graphql, subgraph.yaml, mappings.ts
redstart-test     ModuleTree → native interpreter for `test` blocks (mock store)
redstart-lsp      tower-lsp language server (diagnostics/format/symbols/hover/def)
redstart-cli      the `redstart` binary: new / check / build / test / dev / fmt / lsp
```

The resolved type system (`RTy`, ABI reading) lives in `redstart-checker` and is
shared with codegen, so "what type is this expression" is answered in exactly one
place.

## Design principles (ranked)

1. **Impossible states unrepresentable** — every documented AS footgun is a type
   error or absent from the grammar.
2. **One source of truth** — schema, manifest, and mappings are one language.
3. **Errors teach** — Elm-grade diagnostics are the product.
4. **Feels like the domain** — Solidity-event affinity, entity-centric blocks.
5. **One obvious way** — no `==`/`===`, no integer-type zoo in the surface syntax.
6. **Always ejectable** — emitted AssemblyScript is readable and canonical.

## License

MIT © The Lodestar Team
