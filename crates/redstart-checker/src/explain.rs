//! Human explanations for diagnostic codes — the data behind `redstart explain`.
//!
//! Errors are the product (the "errors teach" principle). Every code carries not
//! just *what* tripped, but *the footgun it prevents* and *the canonical fix*.

/// A teaching explanation for one diagnostic code.
#[derive(Debug, Clone, Copy)]
pub struct Explanation {
    /// The bare code, e.g. `E062`.
    pub code: &'static str,
    /// A short title.
    pub title: &'static str,
    /// What triggers the diagnostic.
    pub summary: &'static str,
    /// The subgraph footgun this makes unrepresentable (empty if purely structural).
    pub prevents: &'static str,
    /// The canonical fix.
    pub fix: &'static str,
}

/// Look up the explanation for a code. Accepts `E062`, `e062`, or
/// `redstart::check::E062`.
#[must_use]
pub fn explain(code: &str) -> Option<&'static Explanation> {
    let bare = code.rsplit("::").next().unwrap_or(code);
    EXPLANATIONS
        .iter()
        .find(|e| e.code.eq_ignore_ascii_case(bare))
}

/// Every known explanation, in code order — for listing.
#[must_use]
pub fn all() -> &'static [Explanation] {
    EXPLANATIONS
}

static EXPLANATIONS: &[Explanation] = &[
    Explanation {
        code: "E001",
        title: "unknown generic type",
        summary: "A generic like `Id<…>` or `Option<…>` used a base type Redstart doesn't know.",
        prevents: "",
        fix: "Use a known generic: `Id<Bytes>`, `Id<String>`, `Option<T>`, or a list `[T]`.",
    },
    Explanation {
        code: "E002",
        title: "unknown type",
        summary: "A field or signature referenced a type that isn't a scalar, entity, enum, or interface.",
        prevents: "",
        fix: "Declare the entity/enum/interface, or use a built-in scalar (Bytes, BigInt, BigDecimal, String, Boolean, Int8, Timestamp).",
    },
    Explanation {
        code: "E003",
        title: "unknown interface",
        summary: "`implements` named an interface that isn't declared.",
        prevents: "",
        fix: "Declare it with `interface Name { … }`.",
    },
    Explanation {
        code: "E004",
        title: "missing interface field",
        summary: "An entity claims to implement an interface but omits one of its fields.",
        prevents: "A schema that the canonical toolchain rejects for an incomplete interface.",
        fix: "Add every field the interface declares to the implementing entity.",
    },
    Explanation {
        code: "E005",
        title: "aggregation sources unknown entity",
        summary: "An `aggregation … over X` referenced an entity `X` that doesn't exist.",
        prevents: "",
        fix: "Point the aggregation at a declared `timeseries` entity.",
    },
    Explanation {
        code: "E010",
        title: "duplicate entity",
        summary: "Two entities share a name across the project.",
        prevents: "Schema/handler ambiguity over which entity a name refers to.",
        fix: "Each entity must be declared exactly once across all modules — rename or remove one.",
    },
    Explanation {
        code: "E020",
        title: "derived field on a non-entity",
        summary: "A `derived from` field must reference an entity type.",
        prevents: "",
        fix: "Derived fields look like `swaps: [Swap] derived from pool`.",
    },
    Explanation {
        code: "E021",
        title: "derived field has no back-reference",
        summary: "The field named by `derived from` doesn't exist on the target entity.",
        prevents: "A runtime `unexpected null` when reading the (non-existent) relation.",
        fix: "Add the referenced back-reference field to the target entity.",
    },
    Explanation {
        code: "E030",
        title: "source missing a required setting",
        summary: "A `source` block is missing a required setting.",
        prevents: "A manifest that fails to deploy.",
        fix: "A source needs `abi`, `network`, `address`, and `startBlock`.",
    },
    Explanation {
        code: "E031",
        title: "template missing a required setting",
        summary: "A `template` block is missing a required setting.",
        prevents: "A manifest that fails to deploy.",
        fix: "Add the missing key (templates need `abi` and `network`).",
    },
    Explanation {
        code: "E032",
        title: "unknown ABI",
        summary: "A source or template referenced an ABI that wasn't imported.",
        prevents: "",
        fix: "Import it: `abi Name from \"./abis/Name.json\"`.",
    },
    Explanation {
        code: "E040",
        title: "unknown source",
        summary: "A handler targets a source or template that isn't declared.",
        prevents: "",
        fix: "Declare it with a `source` (or `template`) block.",
    },
    Explanation {
        code: "E041",
        title: "event not found in ABI",
        summary: "`handler on Src.Event` named an event the ABI doesn't contain.",
        prevents: "A handler that silently never fires because its signature can't keccak-match the chain.",
        fix: "Check the event name and casing against the ABI.",
    },
    Explanation {
        code: "E042",
        title: "function not found in ABI",
        summary: "A call handler bound a contract function the ABI doesn't contain.",
        prevents: "A call handler that silently never fires.",
        fix: "Check the function name against the ABI.",
    },
    Explanation {
        code: "E050",
        title: "unknown entity",
        summary: "A handler referenced an entity type that isn't declared.",
        prevents: "",
        fix: "Declare the entity, or fix the name.",
    },
    Explanation {
        code: "E051",
        title: "incomplete initializer",
        summary: "An entity was created without all of its required (non-`Option`) fields.",
        prevents: "graph-node's `missing value for non-nullable field`, or a silent null-merge.",
        fix: "Initialise every required field at creation, or make the field `Option<T>`.",
    },
    Explanation {
        code: "E052",
        title: "unknown field in initializer",
        summary: "An initializer set a field the entity doesn't declare.",
        prevents: "",
        fix: "Use only fields declared on the entity.",
    },
    Explanation {
        code: "E053",
        title: "assignment to a derived field",
        summary: "Code assigned to a `@derivedFrom` field.",
        prevents: "A silent no-op write — derived fields are virtual and read-only.",
        fix: "Write the back-reference on the child entity instead; the relation derives automatically.",
    },
    Explanation {
        code: "E054",
        title: "unknown field",
        summary: "Code accessed a field the entity doesn't declare.",
        prevents: "",
        fix: "Use a field declared on the entity.",
    },
    Explanation {
        code: "E055",
        title: "`.save()` on an entity",
        summary: "Code called `.save()` on an entity, the AssemblyScript habit Redstart removes.",
        prevents: "Both halves of the classic save bug: the forgotten `.save()` that silently drops a write, and the stale-overwrite from saving a copy loaded earlier.",
        fix: "Delete the call. Redstart dirty-tracks every entity it creates or mutates and saves it at the end of the scope it was declared in — including on an early `return`, and inside a `match` arm.",
    },
    Explanation {
        code: "E056",
        title: "cleared a required field",
        summary: "Code assigned `None` to a field that isn't declared `Option<T>`.",
        prevents: "Writing a null into a non-nullable column — graph-node fails the write with `missing value for non-nullable field` mid-sync.",
        fix: "Declare the field `Option<T>` if it can genuinely be absent, then `entity.field = None` clears it (lowered to graph-ts's `unset`). Otherwise assign a real value.",
    },
    Explanation {
        code: "E057",
        title: "unknown trigger parameter",
        summary: "`event.params.x` (or `call.inputs.x`) named a parameter the ABI's event/function doesn't declare.",
        prevents: "The exact drift Redstart exists to remove: a renamed or mistyped parameter that the Redstart build accepts and only the AssemblyScript compiler rejects — or worse, one that reads a different value than intended.",
        fix: "Use a parameter the ABI declares (the diagnostic lists them). Unnamed ABI inputs are addressed positionally as `param0`, `param1`, … exactly as `graph codegen` names them.",
    },
    Explanation {
        code: "E060",
        title: "`.value` of a contract call read directly",
        summary: "Code touched `.value` of a contract call without matching its `Result`.",
        prevents: "An unhandled reverted `eth_call` aborting the handler — a non-determinism / PoI hazard.",
        fix: "`match call { Ok(v) => { … } Err(e) => { … } }` before using the value.",
    },
    Explanation {
        code: "E061",
        title: "arithmetic on an `Option`",
        summary: "Code did arithmetic on a possibly-absent `Option` value.",
        prevents: "AssemblyScript's nullable-arithmetic miscompile (silently wrong numbers).",
        fix: "Unwrap first: `match` it, or use `.unwrapOr(default)` before the arithmetic.",
    },
    Explanation {
        code: "E062",
        title: "dereference of a nullable value",
        summary: "Code accessed a field on a nullable value — a `load`/`loadInBlock`/`ipfs.cat` result.",
        prevents: "A null dereference that aborts the handler at runtime, three hours into a sync.",
        fix: "`match x { Some(v) => { … } None => { … } }` — these host calls can return nothing.",
    },
    Explanation {
        code: "E070",
        title: "non-exhaustive `match`",
        summary: "A `match` didn't cover every variant.",
        prevents: "An unhandled case slipping through at runtime.",
        fix: "Handle every variant, or add a `_ => { … }` wildcard arm.",
    },
    Explanation {
        code: "E080",
        title: "non-deterministic call",
        summary: "Code called a wall-clock or randomness host function (`Date.now`, `Date.UTC`, `Date.parse`, `Math.random`).",
        prevents: "Non-determinism that diverges Proof-of-Indexing across indexers — a slashing risk; graph-node blocks some of these at runtime.",
        fix: "Use `event.block.timestamp` for time; derive any 'random' value from on-chain data. A subgraph must index identically everywhere.",
    },
    Explanation {
        code: "E090",
        title: "division by zero",
        summary: "Code divided by a value that is statically zero — a `0`/`0.0` literal or `BigInt.zero()`/`BigDecimal.zero()`.",
        prevents: "A fatal, deterministic sync halt: graph-node aborts with `attempted to divide … by zero` and the subgraph stops.",
        fix: "Guard the denominator (`if d != BigInt.zero { … }` / `match`), or divide by a value you know is non-zero.",
    },
    Explanation {
        code: "W030",
        title: "BigInt division loses precision",
        summary: "A `BigInt / BigInt` result is assigned to a `BigDecimal` field — integer division truncates the fraction first.",
        prevents: "The canonical Uniswap bug: a price like ETH/MKR computes to `0` instead of `~0.27`, because the division happened in integer space.",
        fix: "Use `.divDecimal()`, or convert the operands with `.toBigDecimal()` before dividing, so the ratio keeps its fraction.",
    },
    Explanation {
        code: "W010",
        title: "call handler on a network without tracing",
        summary: "A `handler call …` targets a network whose nodes don't expose Parity-style call tracing (Arbitrum, Optimism, Base, Polygon, BNB, …).",
        prevents: "A call handler that silently never fires on that network — you'd index nothing and not know why.",
        fix: "Prefer an event handler. Call/`call`-trace handlers are reliable mainly on Ethereum mainnet.",
    },
    Explanation {
        code: "W020",
        title: "contract call inside a loop",
        summary: "An `eth_call` (a `Contract.bind(...).fn(...)`) appears inside a `for`/`while` loop.",
        prevents: "The classic 'stuck at 3%' sync: each call is a 100 ms+ blocking RPC, run serially while the handler is paused — N iterations means N round-trips.",
        fix: "Hoist the call out of the loop if its result is loop-invariant, or cache it. Reading the same value once and reusing it is dramatically faster.",
    },
    Explanation {
        code: "W040",
        title: "entity id is a stringified address/bytes",
        summary: "An entity is keyed on a single `Bytes`/`Address` value stringified with `.toHexString()` / `.toHex()` — a hex-string id instead of a raw-bytes one.",
        prevents: "Needless indexing cost: a `Bytes` id (with immutability) indexes ~28% faster and stores ~48% less than the equivalent hex-string id (Edge & Node benchmark). Composite ids joined from several values are genuinely strings and are never flagged.",
        fix: "Declare the entity `id: Id<Bytes>` and pass the raw `Bytes`/`Address`, dropping the `.toHexString()` — or let `redstart fix --ids` do it for you (it converts an entity only when every id site is a single stringified value). Note this changes the stored id representation, so re-deploy from the affected block.",
    },
    Explanation {
        code: "W050",
        title: "stored array of entity references",
        summary: "An entity field is a *stored* array of another entity (`[Child]`), not a `derived from` relation. graph-node keeps such arrays inline.",
        prevents: "Quadratic write amplification: every append copies the entire array into a new versioned row, so a growing relation is O(n²) on disk (harmful beyond ~thousands of elements; tolerable only for small, bounded sets). Scalar/enum arrays (`[String]`, `[BigInt]`) are genuinely stored and never flagged.",
        fix: "Model the one-to-many with `@derivedFrom`: add a back-ref field on the child pointing at the parent, then declare the array `field: [Child] derived from <back-ref>`. The reverse lookup is computed on read, never stored — O(1) appends.",
    },
    Explanation {
        code: "W011",
        title: "unfiltered block handler",
        summary: "A `handler block Src` with no `every N` or `once` runs on every block of the entire chain.",
        prevents: "A pathologically slow sync — the handler fires for every block from `startBlock` onward.",
        fix: "Add `every N` to poll every N blocks, or `once` to run a single time.",
    },
    Explanation {
        code: "E071",
        title: "contract has no such function",
        summary: "Code called a function the bound contract's ABI doesn't declare.",
        prevents: "",
        fix: "Check the function name against the ABI (only view/pure calls are supported).",
    },
    Explanation {
        code: "E072",
        title: "call to undefined function",
        summary: "A bare `name(…)` call named no declared `fn`, in this module or any other.",
        prevents: "A half-finished port passing `check` and then failing deep inside `graph build` with an AssemblyScript `Cannot find name` — no Redstart source line, and only after codegen. `check` is the fast inner loop, so it is where an unresolved call has to surface.",
        fix: "Declare the helper with `fn name(…) -> … { … }` — free functions are visible from every module, so there is nothing to import — or fix the spelling. Entities are constructed with `Entity.create(id, { … })`, not by calling the entity name.",
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_is_flexible_and_complete() {
        assert!(explain("E062").is_some());
        assert_eq!(explain("e062").unwrap().code, "E062");
        assert_eq!(explain("redstart::check::E062").unwrap().code, "E062");
        assert!(explain("E999").is_none());
        // Every entry is well-formed.
        for e in all() {
            assert!(e.code.starts_with('E') || e.code.starts_with('W'));
            assert!(!e.title.is_empty() && !e.summary.is_empty() && !e.fix.is_empty());
        }
    }
}
