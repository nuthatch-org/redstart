# Redstart 2026 Roadmap

**From "a nicer AssemblyScript" to *the optimising, footgun-proof compiler for subgraphs*.**

This document is the strategy and backlog for making Redstart not 10% better than
hand-writing AssemblyScript subgraphs, but *categorically* better — safer, faster,
and the obvious default for both humans and AI agents. It is grounded in a cited
catalogue of real, documented AssemblyScript / graph-node footguns (see
[§7 Appendix](#7-appendix--the-cited-footgun-catalogue)), not vibes.

---

## 0. The thesis (read this first)

> **AssemblyScript is a *language*. Redstart is a *compiler that knows it is
> compiling a subgraph*.**

That single distinction is the entire unfair advantage, and almost nothing in the
roadmap below is reachable by "just write AssemblyScript":

1. **AS cannot prevent a subgraph bug it doesn't understand.** It has no concept of
   an entity, a `.save()`, a derived field, a non-deterministic handler, or a
   reverted `eth_call`. Redstart does — so it can make whole bug classes
   *unrepresentable*, not merely *lintable*.
2. **AS cannot optimise your indexing.** It doesn't know an `eth_call` is a 100 ms
   blocking RPC, that a stored array is O(n²) disk, or that your `startBlock` is
   scanning ten million dead blocks. Redstart owns the schema, the manifest, *and*
   the mappings — so it can apply every documented performance best-practice
   automatically.
3. **We own the layer that emits the AssemblyScript.** A human writes *some* AS; we
   write *every line* of it, for every subgraph, from a model that understands what
   it's building. So the generated output isn't "as good as a careful human" — it's
   the **provably-optimal AS no human would bother to hand-write**: the fastest
   id type, the right immutability, declared parallel calls, pruned storage,
   coalesced loads — applied uniformly, every time, with zero developer effort.

> **The unlock (2026-06-27): the kill-gate is green, so we can optimise
> fearlessly.** Every performance rewrite changes the emitted AssemblyScript — and
> the field-level store-diff ([§6](#6-the-kill-gate)) *proves* it produces a
> byte-identical store. So the loop is: **rewrite → `run.sh all` → "0 diffs, but
> 28% faster, 48% less disk."** Nothing else in the subgraph ecosystem has a
> closed loop that lets it change generated code and *prove* the data is identical.
> This is what makes "truly perfect generated code" a buildable goal rather than a
> slogan.

The strongest external evidence for lever #1: The Graph already ships a
[**Subgraph Linter**](https://thegraph.com/docs/en/subgraphs/guides/subgraph-linter/)
with eight named checks — `entity-overwrite`, `unexpected-null`, `unchecked-load`,
`unchecked-nonnull`, `division-guard`, `derived-field-guard`,
`helper-return-contract`, `undeclared-eth-call`. **That linter is a list of things
The Graph admits are footguns but can only *warn* about.** Redstart's mission is to
turn every one of them into a *compile error or an absent grammar rule.*

**The one-line pitch we are building toward:**
*"Redstart makes the Subgraph Linter unrepresentable and emits AssemblyScript that
is faster than what a human would hand-write — automatically, uniformly, and
provably store-identical. Which a language never can."*

---

## 1. Where we are — Stage 0, done (✅)

Shipped and eject-verified (the canonical `graph build` compiles our output to WASM
unmodified), proven end-to-end by porting a real subgraph
([`examples/horizon-indexer`](../examples/horizon-indexer), a port of
[PaulieB14/horizon-indexer-subgraph](https://github.com/PaulieB14/horizon-indexer-subgraph))
and **deploying it live to Subgraph Studio**.

- Unified language: schema + manifest + mappings from one `.red` source, multi-file
- Control flow, helper `fn`s (cross-module, return-typed, `return`-flushed auto-save)
- All handler kinds: event / call / block / file (IPFS)
- Dynamic data sources (templates + context), enums, interfaces, timeseries +
  aggregations, `Int8`/`Timestamp`
- **Footguns already killed by construction:** forgotten `.save()` (auto-save +
  return-flush), nullable host returns (`load`/`loadInBlock`/`ipfs.cat` →
  `Option<T>`, deref is a compile error E062), reverted calls (`Result` + forced
  `match`), arithmetic-on-`Option`, assign-to-derived, manifest/schema/handler drift
- Tooling: native test interpreter (no Docker), LSP, formatter, `dev` watch,
  `deploy`, tree-sitter

**The kill-gate is GREEN (2026-06-27).** The field-level **store-diff** — that our
generated WASM indexes *byte-identically* to hand-written AssemblyScript against a
live graph-node — is proven: `conformance/fixtures/arb-erc20` (the ARB token on
Arbitrum One) deployed alongside the hand-written `reference/erc20` reference and
diffed **0 differences** across 10 Account + 13 Transfer entities at block
477,660,492. Indexing fidelity is no longer a hypothesis. See [§6](#6-the-kill-gate).

---

## 2. The three levers

| Lever | What it is | Why AS can't | Status |
|---|---|---|---|
| **1. Unrepresentable bugs** | Turn the 8 Subgraph-Linter checks + determinism hazards into compile errors / absent grammar | AS has no model of entities, saves, determinism, reverts | **~70% — the high-frequency footguns are dead** (null-deref, option-arith, determinism, div-zero, precision, reverts, derived, auto-save) |
| **2. Optimising compiler** | Apply every documented perf best-practice automatically (startBlock, declared eth_calls, Bytes ids, immutable inference, derived-array rewrite, pruning) | AS doesn't know it's a subgraph | **the live frontier** — barely started, and now *gate-verifiable*. This is the categorical advantage. |
| **3. Agent-native** | Structured diagnostics, in-loop native tests, MCP server, scaffold-from-address | n/a — this is product surface | **~40%** — `--json` + `explain` shipped; MCP + scaffold-from-address pending |

**Where the leverage is now.** Lever 1 makes Redstart *safer* than AS — and the
common cases are done. Lever 2 makes it *faster* than AS, and it's the one a
language fundamentally cannot pull: a hand-written and a Redstart subgraph can be
equally correct, but only Redstart can auto-apply Bytes-ids, declared `eth_calls`,
`@derivedFrom` rewrites and pruning to *every* subgraph. With the kill-gate green,
**Lever 2 is both the most differentiated and the now-safest work left** — so it is
the priority arc from here. The rest of this doc is the backlog, pillar by pillar.

---

## 3. Pillar: Correctness & Security — *make the linter unrepresentable*

Each item maps to a Subgraph-Linter check or a documented incident. Status: ✅ done ·
🔜 next · 📋 planned.

### 3.1 ✅ Forgotten `.save()` / stale-overwrite (`entity-overwrite`)
Auto-save with dirty-tracking + return-flush + matched-binding auto-save. The
classic "helper reloads+saves, handler then saves its stale copy, last write wins"
(linter `entity-overwrite`) is structurally impossible because there is one tracked
instance per (entity, id) per invocation.
*Refs:* [linter](https://thegraph.com/docs/en/subgraphs/guides/subgraph-linter/) ·
[AS mappings](https://thegraph.com/docs/en/subgraphs/developing/creating/assemblyscript-mappings/)

### 3.2 ✅ Nullable derefs (`unexpected-null`, `unchecked-load`, `unchecked-nonnull`)
`load`/`loadInBlock`/`ipfs.cat` return `Option<T>`; touching a field without `match`
is compile error **E062**. There is no `!` non-null override in the grammar to
abuse. **Extend (🔜):** `TypedMap.get` / `json` getters → `Option`; model
`loadInBlock`'s "null even if it exists in store" semantics so it can't be confused
with `load`.
*Refs:* [graph-ts API](https://thegraph.com/docs/en/subgraphs/developing/creating/graph-ts/api/) ·
[derivedFrom null incident](https://github.com/graphprotocol/graph-ts/issues/219)

### 3.3 ✅ Derived fields (`derived-field-guard`)
Assigning to a derived field is rejected; reading one in a handler forces `.load()`.

### 3.4 ✅ Division-by-zero (`division-guard`) — done (v0.8.0, E090)
Divide-by-zero is a **fatal, deterministic sync halt** (`attempted to divide
BigDecimal '86400' by zero`). Make `/` on `BigInt`/`BigDecimal` require a
provably-non-zero denominator (a literal, or a value guarded by a preceding
`if denom != 0` / `match`), else a compile error with a fix-it.
*Refs:* [graph-node #5281](https://github.com/graphprotocol/graph-node/issues/5281) ·
[linter](https://thegraph.com/docs/en/subgraphs/guides/subgraph-linter/)

### 3.5 ✅ BigInt-division precision trap — done (v0.8.0, W030)
`BigInt / BigInt` truncates fractions to **zero** (the canonical Uniswap bug:
ETH/MKR price returns `0` instead of `~0.27`). When a `BigInt` division result flows
into a `BigDecimal` field, *require* `.divDecimal()` — or warn and offer the fix.
*Refs:* [graph-node #720](https://github.com/graphprotocol/graph-node/issues/720)

### 3.6 🔜 BigDecimal 34-digit precision ceiling — **MEDIUM**
`BigDecimal` is decimal128 (34 significant digits); wider fixed-point (`ufixed256x18`)
loses precision silently. Warn statically when a value could exceed the ceiling
(e.g. raw `uint256` token amounts before scaling).
*Refs:* [graph-ts API](https://thegraph.com/docs/en/subgraphs/developing/creating/graph-ts/api/)

### 3.7 ✅→🔜 Determinism — forbid non-determinism in the grammar — **HIGH**
Non-determinism breaks Proof-of-Indexing across indexers and gets them slashed.
graph-node already *blocks* `Math.random` (`unknown import: env::seed`). **Done
(v0.4.0):** `Date.now`/`Date.UTC`/`Date.parse`/`Math.random` calls are a compile
error (**E080**) with a fix-it pointing at `event.block.timestamp`; the surface
language has no float type to begin with. **Next:** unguarded
`assert`/array-length assumptions are a real divergence vector (the 28-ids-34-values
`TransferBatch` incident that halted one indexer while another reported 100% synced).
*Refs:* [determinism contract](https://thegraph.com/docs/en/subgraphs/developing/creating/advanced/) ·
[randomness blocked](https://forum.thegraph.com/t/indexing-error-subgraph-studio/5354) ·
[divergence incident](https://forum.thegraph.com/t/the-integrity-of-the-data-may-be-open-to-question/4470)

### 3.8 🔜 Entity-ID collisions — **HIGH**
Naive string-concat IDs lose field boundaries (`"ab"+"c"` == `"a"+"bc"`) and
tx-hash-only IDs collide when one tx emits N events → silent under-counting. Provide
typed, collision-resistant ID composition (`bytes.concatI32(logIndex)`), and
**forbid raw string `+` for IDs** — point users at the typed builder.
*Refs:* [bytes-as-ids](https://thegraph.com/docs/en/subgraphs/best-practices/immutable-entities-bytes-as-ids/)

### 3.9 🔜 Event-signature / topic0 drift — **HIGH**
A handler **silently never fires** if its event signature doesn't keccak-match the
on-chain one (and graph-node downgraded the "Skipping handler" log to `trace` — it's
invisible). codegen also mis-emits `tuple[]` for struct-array events so topic0 never
matches. We already derive signatures from the ABI by reference — **extend to expand
tuple/struct components** and verify indexed/non-indexed placement, so drift is a
compile error, not a silent no-op.
*Refs:* [manifest](https://thegraph.com/docs/en/subgraphs/developing/creating/subgraph-manifest/) ·
[tuple codegen #520](https://github.com/graphprotocol/graph-tooling/issues/520)

### 3.10 🔜 `eth_call` revert safety (`undeclared-eth-call` + reverts)
Already: contract calls are `Result`, must `match`. **Add:** revert detection is
*client-dependent* (Geth/Infura may miss reverts a Parity client catches) — a genuine
PoI-divergence vector; surface this as a deploy-time warning. The call-handler
"phantom data" incident (a reverted call inside a Safe `multiSend` was indexed as
real) argues for steering hard toward event handlers.
*Refs:* [client-dependent reverts](https://thegraph.com/docs/en/subgraphs/developing/creating/graph-ts/api/) ·
[phantom call #3701](https://github.com/graphprotocol/graph-node/issues/3701)

### 3.11 📋 Immutable / timeseries misuse
`@entity(timeseries:true)` auto-manages `id`/`timestamp` (silently overrides if you
set them) and is *always* immutable. Marking a mutable entity immutable traps you
forever. Make auto fields unassignable and reject updates to immutable/timeseries
entities at compile time.
*Refs:* [aggregations](https://github.com/graphprotocol/graph-node/blob/master/docs/aggregations.md)

---

## 4. Pillar: Performance — *be an optimising compiler*

This is the lever AS physically cannot pull, and **the priority arc** (see §2). Legend:
**[AUTO]** compiler rewrites it · **[WARN]** compiler flags it. Priority order (highest
leverage first).

> **Every `[AUTO]` rewrite ships with a proof.** Because we own codegen, an
> optimisation is just a different — better — lowering of the same `.red`. The green
> kill-gate ([§6](#6-the-kill-gate)) turns "trust me, it's equivalent" into a
> CI-checkable fact: apply the rewrite, run `run.sh all`, and the store-diff must
> still read **0 differences**. Optimise the generated AssemblyScript as aggressively
> as the data allows — and prove, every time, that the indexed result is byte-identical.

### 4.1 🔜 [AUTO] Auto-fill `startBlock` from the deployment block — **HIGHEST single win**
Omitting / lowballing `startBlock` scans millions of dead blocks; too high silently
skips real events. The deploy block is deterministically discoverable from the
contract's creation tx. Fetch it and default `startBlock`; **error loudly on `0`**.
*Refs:* [manifest](https://thegraph.com/docs/en/subgraphs/developing/creating/subgraph-manifest/) ·
[#2007](https://github.com/graphprotocol/graph-node/issues/2007)

### 4.2 🔜 [AUTO] Declare `eth_calls` for parallel pre-fetch
`eth_calls` are the **#1 sync killer** (100 ms–several seconds *each*, run serially
while the handler is paused). **Done (v0.6.0):** **[WARN] W020** on any contract
call inside a `for`/`while` loop (the "stuck at 3%" pattern). **Still to do:** when
a call's receiver+args are pure functions of `event.address`/`event.params`, emit a
manifest `calls:` block (specVersion ≥ 1.2.0) so graph-node runs them **in parallel
before handlers** and caches them (Goldsky figure: 5–10×); **[AUTO]** rewrite a call
whose return is already in the triggering event → read the event param; **[WARN]** on
metadata calls (`name`/`symbol`/`decimals`) not behind a `load()==null` cache.
*Refs:* [avoid eth_calls](https://thegraph.com/docs/en/subgraphs/best-practices/avoid-eth-calls/) ·
[reduce eth_calls](https://thegraph.com/blog/improve-subgraph-performance-reduce-eth-calls/) ·
[declared calls (Goldsky)](https://docs.goldsky.com/subgraphs/guides/declared-eth-calls)

### 4.3 ✅ [AUTO/WARN] Infer `immutable: true` (v0.9.0) · Bytes-ids (W040 v0.11.0 → `fix --ids` v0.12.0)
Measured (Edge & Node benchmark): immutable + Bytes ids → **28% faster indexing,
48% less disk** vs mutable + String ids. **✅ [WARN] (v0.11.0):** W040 flags an id
built by stringifying a single `Bytes`/`Address` (`.toHexString()`/`.toHex()`,
directly or via a local) and recommends `id: Id<Bytes>`. Deliberately a *warning*,
not an auto-rewrite: a String→Bytes id changes the stored id value (hex-string → raw
bytes), so — unlike immutability, which is store-identical — it is a data change the
author opts into (re-deploy from the affected block). Composite keys and literal-string
ids are never flagged. **✅ [AUTO] (v0.12.0, extended v0.14.0):** `redstart fix --ids`
performs the conversion — flips the declaration to `Id<Bytes>` and drops the `.toHexString()`
at every site in one pass — but only when *every* id site of the entity is a single stringified
value (one literal/composite site and the whole entity is reported and left untouched, so it
never emits code that fails to check). v0.14.0 also handles the common `let id = x.toHexString();
E.create(id, …)` shape, when `id` is used exactly once. `--dry-run` previews. When a type
is only ever `new`-constructed +
`.save()`d (never load-then-mutate anywhere), emit `@entity(immutable: true)`.
(Redstart already does this for timeseries — generalise it.) Caveat: figures are "up to"
and workload-dependent ([#3534](https://github.com/graphprotocol/graph-node/issues/3534)
shows a regression case) — so gate behind a confidence check / opt-out.
*Refs:* [bytes-as-ids](https://thegraph.com/docs/en/subgraphs/best-practices/immutable-entities-bytes-as-ids/) ·
[benchmark](https://medium.com/edge-node-engineering/two-simple-subgraph-performance-improvements-a76c6b3e7eac)

### 4.4 ✅→🔜 [WARN/AUTO] Growing stored array → `@derivedFrom` (W050, v0.13.0)
A stored `[Child!]!` mutated via `.push()`+`.save()` is O(n²) disk (every append
copies the whole array into a new versioned row; harmful beyond ~thousands, tolerable
< ~50). **✅ [WARN] (v0.13.0):** W050 flags an entity field `[Child]` (a stored array
of another entity, not `derived from`) and steers to `@derivedFrom` with a suggested
back-ref name. Scalar/enum arrays never fire; the `derived from` form stays clean.
Like W040 it's a warning not an auto-rewrite — the migration needs an author-chosen
back-ref field and changes the stored data model. **🔜 [AUTO] next:** synthesise the
migration when the back-ref is unambiguous — add the child field, annotate the parent
`@derivedFrom`, rewrite the array mutation to `child.save()`.
*Refs:* [derivedFrom](https://thegraph.com/docs/en/subgraphs/best-practices/derivedfrom/) ·
[avoid large arrays](https://thegraph.com/blog/improve-subgraph-performance-avoiding-large-arrays/)

### 4.5 ✅ [AUTO] Default `indexerHints: prune: auto` (v0.10.0)
Absent `indexerHints`, the default is `prune: never` (largest DB, slowest queries).
Default to `prune: auto`; **[WARN]** and downgrade to `prune: <N>`/`never` when
grafting or time-travel queries are detected (pruning breaks both).
*Refs:* [pruning](https://thegraph.com/docs/en/subgraphs/best-practices/pruning/)

### 4.6 🔜 [AUTO] Coalesce redundant loads; hoist loop-invariant loads
Each `load(id)` is a SELECT + (on save) a versioned write. Coalesce repeated
load/save of the same id, hoist loop-invariant loads, and substitute `loadInBlock`
**only** when intra-block dataflow proves same-block creation (its null-semantics
differ, so it's unsafe otherwise).
*Refs:* [graph-ts API](https://thegraph.com/docs/en/subgraphs/developing/creating/graph-ts/api/)

### 4.7 ✅→🔜 [WARN] Handler-shape lints
**Done (v0.5.0):** warning **W011** on an unfiltered block handler (runs every
block of the whole chain), and **W010** on a `handler call` whose source network
has no Parity tracing (Arbitrum/Optimism/Base/Polygon/BNB — the handler silently
never fires there). This also introduced **warning-severity diagnostics** (a
`.warning()` builder; warnings report but don't fail the build), unlocking the
rest of the §4 `[WARN]` lints. **Next:** mismatched `startBlock`s across
call-handler sources (graph-node `trace_filter` blowup, 4 h build → 24 h index).
*Refs:* [manifest](https://github.com/graphprotocol/graph-node/blob/master/docs/subgraph-manifest.md) ·
[#2007](https://github.com/graphprotocol/graph-node/issues/2007)

### 4.8 📋 [AUTO] Suggest timeseries/aggregations for load-mutate-save totals
A hand-rolled rolling sum (`load` totals entity → mutate → `save` per event) is a
write-contention hotspot. When that pattern appears, suggest a DB-computed
`@aggregation` (zero handler code). We already support the feature — add the
detection.
*Refs:* [timeseries](https://thegraph.com/docs/en/subgraphs/best-practices/timeseries/)

---

## 5. Pillar: Agent-native DX & developer lovability

Redstart should be the language an AI agent *prefers* to write subgraphs in — and
the one a human falls in love with. These overlap.

### 5.1 ✅ Structured, machine-readable diagnostics (`--json`) — **HIGHEST agent leverage**
`redstart check --json` emits `{ "ok": bool, "diagnostics": [ {code, severity,
message, label, help, file, line, column, offset, length}, … ] }` to stdout
(exit non-zero when not `ok`), so an agent loop *reads the error and applies the
fix* without parsing prose. Lex/parse/resolution failures are emitted too (ANSI
stripped). Elm-grade errors aren't just nice for humans — they're the agent's
feedback signal. **Next:** a `fix` field carrying a machine-applicable edit (span
+ replacement) once the auto-fixes in §3–4 land.

### 5.2 ✅→🔜 Native test interpreter as an in-loop tool
Already a moat over Matchstick (no Docker/WASM, sub-second). Keep investing: richer
assertions, fixture helpers, coverage. An agent can write-and-run a test in the same
turn — Matchstick can't touch that loop time.

### 5.3 🔜 MCP — two complementary layers
There are **two** MCP layers in a Redstart agent story; they don't compete.

- **Author-side (`redstart mcp`) — ✅ shipped (v0.15.0).** Exposes `check` /
  `explain` / `build` / `test` as MCP tools over stdio (hand-rolled JSON-RPC 2.0,
  new `redstart-mcp` crate) so any agent (Claude Code, Cursor, …) *writes and
  verifies* a Redstart subgraph in-loop. `check` is the keystone: it returns the
  same structured diagnostics as `check --json` (errors + lint warnings, precisely
  located), and load/parse failures come back as `{ ok: false, diagnostics }` so
  the loop never stalls. `new-from-address` is deferred (network-gated, §5.4).
  The natural payoff of the `--json` + `explain` groundwork.
- **Consumer-side — adopt, don't rebuild.** [`graphops/subgraph-mcp`](https://github.com/graphops/subgraph-mcp)
  (Rust; *query* deployed subgraphs — search, schema, execute GraphQL) and
  `subgraph-registry-mcp` (npm; *discover* subgraphs) already exist and are
  maintained. An agent authoring a subgraph wants these too — to find a reference
  deployment, crib a schema, and **verify its freshly-deployed Redstart subgraph
  returns the right data**. Recommend them as companions now; only **fork
  `graphops/subgraph-mcp` into `nuthatch-org`** once we want a Redstart-specific
  tool such as *"diff my live deployment against my `.red` source"* — a natural
  extension of the kill-gate, but step 2, not step 1.

### 5.4 🔜 `redstart new --from 0x<address> [--network …]` — the no-brainer on-ramp
Fetch the ABI + deployment block (Etherscan/Sourcify), scaffold a working starter
subgraph (entities inferred from events, one handler per event, sensible
`startBlock`). Turns "start a subgraph" from an afternoon into ten seconds — for
humans *and* agents. This is probably the single biggest adoption lever in the doc.

### 5.5 ✅ `redstart explain E062` / inline fix-its
`redstart explain <CODE>` explains any diagnostic code — title, what triggered
it, **the bug it prevents**, and the canonical fix (`--json` too; bare `redstart
explain` lists all 24 codes). Done in v0.3.0. **Next:** inline fix-its as LSP
code actions once the auto-fixes in §3–4 land.

### 5.6 📋 LSP completeness + editor reach
Code actions (apply the auto-fixes from §3–4 as quick-fixes), inlay hints for
inferred types, rename, find-references; ship the VS Code extension to the
marketplace; tree-sitter to nvim/Helix/Zed/GitHub.

### 5.7 📋 One-binary install + great `--help`
`curl | sh`, Homebrew, prebuilt binaries (already drafted in the README). Frictionless
install is table stakes for lovability.

---

## 6. The kill-gate ✅ GREEN

`./conformance/run.sh all` deploys our generated subgraph *and* an idiomatic
hand-written reference to a real graph-node and field-level-diffs their stores at a
fixed block. Everything else proves *it compiles* and *the interpreter agrees*; only
this proves *it indexes identically*.

**Proven 2026-06-27** against `conformance/fixtures/arb-erc20` (ARB token, Arbitrum
One) at block 477,660,492: **0 diffs** across 10 Account + 13 Transfer entities — our
lowered AssemblyScript produced a store identical to the independent hand-written
reference. The asterisk is off every claim in this document.

Reproduce: `RPC_URL=<arbitrum-archive> NETWORK=arbitrum-one
PROJECT=conformance/fixtures/arb-erc20 BLOCK=477660492 ./conformance/run.sh all`
(after `docker compose -f conformance/docker-compose.yml up -d`). The
[`conformance-storediff.yml`](../.github/workflows/conformance-storediff.yml)
workflow runs it in CI once an `RPC_URL` secret is set. **Next:** add hand-written
references for `factory` / `horizon-indexer` to widen the gate.

---

## 7. Prioritised backlog (the order to actually build)

### Done (v0.1.0 → v0.8.0)
- ✅ **Store-diff kill-gate GREEN** (§6) — the bet is proven.
- ✅ `--json` diagnostics (§5.1) + `redstart explain` (§5.5) — agent loop unblocked.
- ✅ Determinism E080 (§3.7); division-guard E090 + BigInt→Decimal precision W030
  (§3.4/3.5); handler-shape lints W010/W011 (§4.7) + warning severity;
  `eth_call`-in-loop W020 (§4.2 partial); ABI `anonymous` normalisation.
- ✅ Editor + distribution maturity: VS Code + Zed extensions, playground, docs site,
  release/CI/snapshot pipelines.

### NEXT ARC — Lever 2, the optimising compiler (the categorical advantage)
*All offline-doable, all provable store-identical via the green gate. Ship each as
a release, each with a `run.sh all` "0 diffs but N% faster" proof.*
1. ✅ **`immutable` inference (§4.3, v0.9.0)** — auto-marks append-only entities
   `@entity(immutable: true)` (created-but-never-loaded/mutated); consistent with the
   gate-proven erc20 annotations. ✅ **Bytes-id half (§4.3, v0.11.0)** — W040 flags a
   single address/bytes stringified into the id, steering to `Id<Bytes>` (~28%/48%).
   ✅ **`fix --ids` auto-conversion (§4.3, v0.12.0)** — opt-in, gated on all-sites-convertible.
2. **Stored-array → `@derivedFrom` (§4.4)** — O(n²) → O(n) disk. ✅ **W050 warn (v0.13.0)**; AUTO synthesis next.
3. ✅ **`prune: auto` default (§4.5, v0.10.0)** — smaller DB, faster queries, for free.
4. **Load coalescing / loop-invariant hoist (§4.6)**.

### Then — Lever 3 author-side MCP + remaining Lever-1 bugs
5. ✅ **`redstart mcp` author-side server (§5.3, v0.15.0)** — the agent-distribution keystone.
6. Collision-resistant typed IDs (§3.8); tuple/struct event-signature expansion (§3.9);
   BigDecimal-ceiling warning (§3.6); LSP code actions (§5.6).

### Network/credential-gated (need a key or confirmation)
7. `redstart new --from 0x<address>` (§5.4) — biggest adoption lever; Etherscan/Sourcify.
8. Auto `startBlock` from deployment block (§4.1); auto-*declare* `eth_calls` for
   parallel prefetch (§4.2, the manifest `calls:` block — 5–10×).

### Widen the proof
9. Hand-written references for `factory` / `horizon-indexer` under
   `conformance/reference/`, so the gate covers more of the surface than ERC-20.

---

## 8. Appendix — the cited footgun catalogue

The research underpinning §3–4, condensed. Every claim was verified against a primary
source; lower-confidence items are flagged. Full reports were produced by web
research against The Graph docs, forum, graph-node/graph-tooling GitHub, Messari
subgraph standards, and the Edge & Node engineering benchmark.

### 8.1 Non-determinism (PoI divergence → slashing)
- Unhandled `eth_call` revert aborts the handler — *"accessed value of a reverted
  call"* · [#5534](https://github.com/graphprotocol/graph-node/issues/5534)
- **Revert detection is client-dependent** (Geth/Infura miss reverts Parity catches) —
  two honest indexers diverge on identical code ·
  [graph-ts API](https://thegraph.com/docs/en/subgraphs/developing/creating/graph-ts/api/)
- `Math.random` blocked at instantiation (`env::seed`) ·
  [forum](https://forum.thegraph.com/t/indexing-error-subgraph-studio/5354)
- `Date.now`/float — *inferred* from the determinism contract (medium confidence)
- Fulltext search "not yet deterministic", panics on rewind ·
  [#5607](https://github.com/graphprotocol/graph-node/issues/5607)
- Unguarded `assert`/array-length divergence incident ·
  [forum](https://forum.thegraph.com/t/the-integrity-of-the-data-may-be-open-to-question/4470)

### 8.2 Entity IDs
- String ids ~2× storage + locale-aware comparison; Bytes+immutable = **28% / 48%** ·
  [bytes-as-ids](https://thegraph.com/docs/en/subgraphs/best-practices/immutable-entities-bytes-as-ids/)
- Concatenation collisions / tx-hash-only collisions → silent under-count ·
  [protofire](https://medium.com/protofire-blog/subgraph-development-part-2-handling-arrays-and-identifying-entities-30d63d4b1dc6)

### 8.3 `@derivedFrom`
- Read-only virtual; reading in a handler → runtime `unexpected null` ·
  [graph-ts #219](https://github.com/graphprotocol/graph-ts/issues/219)
- Unbounded stored arrays = O(n²) disk (time-travel copies the whole array per save) ·
  [avoid large arrays](https://thegraph.com/blog/improve-subgraph-performance-avoiding-large-arrays/)
- `loadInBlock` returns null for cross-block entities ·
  [graph-ts API](https://thegraph.com/docs/en/subgraphs/developing/creating/graph-ts/api/)

### 8.4 Save / partial updates
- Forgotten `save()` (silent loss); stale-overwrite (`entity-overwrite`); array
  getters return *copies* (`.push` doesn't persist); null-merge on unset required
  fields · [linter](https://thegraph.com/docs/en/subgraphs/guides/subgraph-linter/) ·
  [Messari ERRORS.md](https://github.com/messari/subgraphs/blob/master/docs/ERRORS.md)

### 8.5 eth_call + handlers
- ~100 ms–seconds each, serial; declare in manifest (specVersion 1.2.0) for parallel
  pre-fetch + caching · [reduce eth_calls](https://thegraph.com/blog/improve-subgraph-performance-reduce-eth-calls/)
- Call/block handlers "significantly slower", need tracing, unsupported on several
  chains; phantom-data incident ([#3701](https://github.com/graphprotocol/graph-node/issues/3701));
  `trace_filter` blowup ([#2007](https://github.com/graphprotocol/graph-node/issues/2007))

### 8.6 BigInt / BigDecimal
- `BigInt / BigInt` truncates to 0 (Uniswap case) ·
  [#720](https://github.com/graphprotocol/graph-node/issues/720)
- BigDecimal 34-sig-digit ceiling (silent precision loss) ·
  [graph-ts API](https://thegraph.com/docs/en/subgraphs/developing/creating/graph-ts/api/)
- Divide-by-zero = fatal sync halt ·
  [#5281](https://github.com/graphprotocol/graph-node/issues/5281)

### 8.7 Schema / manifest drift
- Handler silently never fires on signature/topic0 mismatch ·
  [manifest](https://thegraph.com/docs/en/subgraphs/developing/creating/subgraph-manifest/)
- Tuple/struct events get malformed codegen signature ·
  [graph-tooling #520](https://github.com/graphprotocol/graph-tooling/issues/520)
- `missing value for non-nullable field` across graph-cli versions ·
  [#5026](https://github.com/graphprotocol/graph-node/issues/5026)

### 8.8 Performance levers (measured / documented)
- Edge & Node benchmark (baseline mutable+String 268 ms/block): Bytes 225 ms (16%),
  immutable 217 ms (19%), both 194 ms (28%); storage 143→74 GB (48%) ·
  [benchmark](https://medium.com/edge-node-engineering/two-simple-subgraph-performance-improvements-a76c6b3e7eac)
- Auto `startBlock` = highest single win · `prune: auto` default · declared calls
  (5–10× vendor figure) · timeseries/aggregations push rollups into the DB

### 8.9 The Subgraph Linter (our compile-error target list)
`entity-overwrite` · `unexpected-null` · `unchecked-load` · `unchecked-nonnull` ·
`division-guard` · `derived-field-guard` · `helper-return-contract` ·
`undeclared-eth-call` ·
[subgraph-linter](https://thegraph.com/docs/en/subgraphs/guides/subgraph-linter/)

**Confidence notes:** the hard performance numbers (16/19/28/48%, 268→194 ms,
143→74 GB) come from a single Edge & Node benchmark and are "up to" / workload-
dependent (counter-example: [#3534](https://github.com/graphprotocol/graph-node/issues/3534)).
The 5–10× declared-calls figure is vendor-stated (Goldsky), not first-party.
`Date.now`/float determinism and a couple of Messari sidechain/timing items are the
lowest-confidence (inferred / search-surfaced) — everything else is verified against
a directly-fetched primary source.

---

*Owner: the Lodestar team. This roadmap is a living document — Stage 0 is done; the
2026 goal is to make "use Redstart" a no-brainer by turning the linter into compile
errors, shipping an optimising compiler, and being agent-native by default.*
