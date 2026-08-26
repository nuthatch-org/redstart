# Porting an existing subgraph

Three handwritten artifacts become one Redstart project. `schema.graphql`
becomes `entity` declarations, `subgraph.yaml` becomes `source` and `template`
blocks, and the AssemblyScript mappings become `handler` bodies. What follows is
the order to do it in, the translation tables, and the places where a mechanical
translation will not work.

Do it in this order. Schema first, because everything else references it; then
sources; then one handler at a time, running `redstart check` after each. Leave
the receipt-dependent handlers for last. They need redesigning rather than
translating, and they are covered at the end.

## Finish with `verify`, not `build`

```sh
redstart check     # is the Redstart valid?
redstart build     # emit schema.graphql + subgraph.yaml + mappings.ts
redstart verify    # …and prove the generated AssemblyScript compiles to WASM
```

`check` and `build` answer questions about your *source*. Only `verify` runs the
canonical `graph codegen` + `graph build` over the output, and that is the
question which decides whether a deploy will work. Run it before every deploy and in CI. It
needs Node and npm; the toolchain is installed into the build directory the first
time. (`redstart deploy <name> --dry-run` does the same thing and then stops.)

The generated directory is output, not source. Add it to `.gitignore` (`redstart
new` does this for you) and never edit it, since the next build overwrites it.

Write the handlers first and the helpers after, if that is how you think. A call
to a `fn` you have not written yet is `E072` at the call site, so `check` tells
you what is still outstanding rather than letting it through to fail inside
`graph build`. Helpers are visible from every module in the project; there is
nothing to import.

## Schema: types

| `schema.graphql` | Redstart |
|---|---|
| `id: ID!` | `id: Id<Bytes>` (preferred) or `id: Id<String>` |
| `field: BigInt!` | `field: BigInt` |
| `field: BigInt` (nullable) | `field: Option<BigInt>` |
| `field: Boolean!` | `field: Boolean` (or `Bool`, both are accepted) |
| `field: Int!` / `Int8!` | `field: Int` / `Int8` |
| `field: [String!]!` | `field: [String]` |
| `field: Token!` | `field: Token` |
| `field: [Trove!]! @derivedFrom(field: "owner")` | `field: [Trove] derived from owner` |
| `@entity(immutable: true)` | `entity X immutable { … }` |
| `type X @entity { … } implements Y` | `entity X implements Y { … }` |

Everything is non-null unless you write `Option<T>`; there is no `null` in the
language. A `derived from` field is read-only, and assigning to one is a compile
error rather than a silent no-op.

Keep your existing id formats if a frontend queries them. Redstart will warn
(`W040`) that a stringified address would be cheaper as `Id<Bytes>`, and it is
right, but changing an id changes entity identity and breaks existing queries.
Accepting that warning on compatibility-sensitive entities is a legitimate
choice; `redstart explain W040` spells out the trade.

## Mappings: idioms

| AssemblyScript | Redstart |
|---|---|
| `let e = Entity.load(id); if (e == null) { e = new Entity(id); … }` | `let e = Entity.loadOrCreate(id, { … })` |
| `new Entity(id)` | `Entity.create(id, { … })` |
| `entity.save()` | nothing at all: entities auto-save (`E055` if you write it) |
| `a.plus(b)` / `a.minus(b)` / `a.times(b)` / `a.div(b)` | `a + b` / `a - b` / `a * b` / `a / b` |
| `let r = c.try_f(x); if (!r.reverted) { r.value … }` | `match C.bind(addr).f(x) { Ok(v) => { … } Err(e) => { … } }` |
| `let e = Entity.load(id); if (e != null) { e.field … }` | `match Entity.load(id) { Some(e) => { … } None => { … } }` |
| `entity.field = null` | `entity.field = None` (the field must be `Option<T>`) |
| `x ? a : b` | `let v = b` then `if cond { v = a }`, there is no ternary |
| `const ZERO = BigInt.fromI32(0)` | a helper: `fn zero() -> BigInt { return BigInt.zero }` |
| `Template.create(addr)` | `Template.create(addr)` (unchanged) |

`load` returns `Option<T>` and a contract call returns `Result<T, _>`; both must
be `match`ed before use, which is what makes the null-deref and the
reverted-call abort unrepresentable. Control flow (`if` / `while` / `for`) and
free `fn` helpers lower to the obvious AssemblyScript.

## What a handler can see

An event handler's `event` binding carries exactly this:

| Available | |
|---|---|
| `event.params.<name>` | the decoded parameters, named as the ABI names them (unnamed inputs are `param0`, `param1`, …). A name the ABI doesn't declare is `E057`. |
| `event.address` | the contract that emitted the event |
| `event.id` | a unique id derived from the transaction hash and log index |
| `event.logIndex`, `event.transactionLogIndex` | position within the block / transaction |
| `event.block.number`, `.timestamp`, `.hash` | the block |
| `event.transaction.hash`, `.from`, `.to`, `.value`, `.gasPrice` | the transaction |
| `dataSource.address()`, `dataSource.context()`, `dataSource.network()` | the data source, including template context |

**Not available: `event.receipt`.** Redstart does not expose the transaction
receipt, so a handler cannot read neighbouring logs, scan the transaction for
another contract's topic, or decode a log at a fixed offset. If the subgraph
you're porting does any of that, it needs the redesign below.

Call handlers see `call.inputs.<name>`, `call.outputs.<name>`, `call.block` and
`call.transaction`. Note that call handlers need Parity-style tracing, which most
L2s do not provide, `W010` warns when the network can't support them.

## Replacing receipt inspection

The pattern that replaces it: **handle each event directly, and correlate them
through an entity keyed by transaction hash.**

Concretely, where the old mapping read a future log from inside a handler:

1. the first handler writes what it knows into a staging entity whose id contains
   `event.transaction.hash` (plus any discriminator, a collateral index, a
   borrower, a batch manager);
2. the handler for the event that used to be read out of the receipt runs
   normally when graph-node reaches it, loads the staging entity, and completes
   the work.

This is strictly more robust than a `logIndex + 2` assumption, which breaks the
moment the contract emits an extra log. It does rely on the two events landing in
the expected order within the transaction, so write that invariant down and test
it, a contract change can violate it.

Two things to decide deliberately. Staging entities are implementation detail
rather than public API, so name them accordingly (`PendingBatchUpdate`,
`PendingLiquidation`) and document that consumers should ignore them. And if more
than one occurrence per transaction is possible, key or accumulate accordingly -
a single pending record silently overwrites.

## Reading other contracts

Declare the ABI and bind it. Nothing else is needed: the ABI is added to the
manifest and its contract class imported wherever it's bound, including from
inside a helper `fn`.

```red
abi CollateralRegistry from "./abis/CollateralRegistry.json"

fn tokenFor(registry: Address, index: BigInt) -> Bytes {
  match CollateralRegistry.bind(registry).getToken(index) {
    Ok(token) => { return token }
    Err(e) => { return Bytes.empty }
  }
}
```

`bind` takes an `Address`, so a helper that binds should declare its parameter
`Address` rather than `Bytes` (`event.address` and `dataSource.address()` are
already `Address`).

Earlier versions needed an inert template with a dummy block handler to make the
contract class appear. That is no longer necessary; delete those declarations.

Contract calls are the main sync-speed lever: each one is a blocking RPC round
trip. `W020` flags a call inside a loop, which is the classic "stuck at 3%"
shape. Hoist it or cache the result.

## Several sources, one event name

Perfectly fine. Three registry contracts all handling
`CollSurplusPoolAddressChanged` generate three distinct exported functions -
`handleAddressesRegistrySource0CollSurplusPoolAddressChanged` and so on, with
the manifest pointing each source at its own. Handler symbols stay unqualified
(`handleTransfer`) when there is nothing to disambiguate.

## Multi-network deployment

The manifest carries one network, taken from each `source` block. To deploy the
same subgraph to several networks, keep the addresses and start blocks in your
own configuration and rewrite the `source` blocks before building, or generate
per-network project directories. `graph build --network` and `networks.json`
still work on the generated output, since it is an ordinary subgraph.

Start blocks matter more than they look. Prefer the block of the initialisation
event you depend on: a `once` block handler that calls a contract can run before
that contract has been initialised, and on a local chain it can run before the
address exists at all. Handling the address-change event directly works on both
historical and fresh deployments.

## Things that will not translate

- **Receipts.** Covered above.
- **Dynamic array sizing from context.** Arrays are written as literals
  (`[0, 0, 0]`), so a length that depends on runtime context has to be handled
  explicitly, build the array in a loop, or accept the fixed length and assert
  the invariant that justifies it.
- **A ternary expression.** Use an `if`.
- **Module-level constants.** Use a helper `fn`.
- **Fulltext search, grafting, and manifest `features`.** Not yet emitted.

## When you find a gap

`redstart check` catching a mistake is the good case, and `redstart verify`
failing is the acceptable one. A generated mapping that fails `graph build`
without either of them complaining is a bug in Redstart, please
[open an issue](https://github.com/nightswatchhq/redstart/issues) with the
smallest `.red` file that reproduces it.
