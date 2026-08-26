# Changelog

All notable changes to Redstart are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); the release workflow
pulls the section matching each tag into the GitHub Release notes.

## [Unreleased]

Found by porting the PancakeSwap Infinity CL subgraph (a real 2,097-line
AssemblyScript subgraph, not an example) in `nightswatchhq/pancakeswap-infinity-cl-redstart`.

### Fixed
- **`Some(x)` was emitted as a call to an undefined function.** An `Option<T>` lowers to graph-ts's `T | null`, so `Some` is the identity constructor. It fell through to the generic call path instead and emitted `Some(x)` literally, which `check` accepted and only `graph build` rejected. `None` was already handled.
- **Matching an `Option` emitted a loose `!= null`, crashing the AssemblyScript compiler.** graph-ts gives `BigInt`, `BigDecimal`, `Bytes` and `Address` an `@operator('!=')` taking two of themselves, so comparing one to `null` sends `asc` into `compileBinaryOverload`, where it fails an internal assertion and aborts the whole build with no line number and no recoverable diagnostic. Null tests are now strict `!==`, which skips overload resolution and is the correct test for a nullable reference. Any subgraph with a nullable `BigInt` field - a nullable `tick` is the common one - hit this.
- **`Entity.create(…)` / `Entity.loadOrCreate(…)` only lowered in `let` position.** As a bare statement (`Swap.create(id, { … })`, the "record this event and move on" shape) or in a `return`, it emitted `Entity.create(id, /* record */)` - a static graph-ts does not have, with the record literal replaced by a comment. Both positions now expand into a temporary, guarded so that `<Template>.create(addr)` still spawns a data source: in statement position the two are textually identical.

### Added
- **`graft { base, block }`.** Start from another deployment's entity store instead of indexing from scratch, which is what a port of a live subgraph almost always wants: re-syncing a busy chain from deployment takes days. Emits both the `graft:` block and `features: [grafting]`, since graph-node rejects a graft that is not declared as a feature. Previously listed in `porting.md` under *Things that will not translate*. Note that only graph-node validates a graft, at deploy time, and only a node already holding the base can perform one — Subgraph Studio holds only what is deployed to Studio and refuses with `the graft base is invalid: deployment not found`.
- **`mutable` on an entity, to decline the append-only inference.** The §4.3 optimisation infers `@entity(immutable: true)` for an entity nothing loads back. That changes graph-node's storage layout, so a schema that must stay compatible with a graft base could not use it — and had no way to say so. `mutable` opts out, `immutable` still opts in, and a timeseries cannot opt out and now says so rather than silently ignoring it.
- **`E006`, unknown entity modifiers.** They were silently ignored, which was tolerable until `mutable` existed: a misspelled `mutible` left the inference in place, turned the entity immutable, and made the subgraph un-graftable with no diagnostic anywhere. Unknown modifiers are now an error with a "did you mean" suggestion, and `mutable` + `immutable` / `mutable` + `timeseries` are rejected as contradictory.
- **`E072`, call to undefined function.** `redstart check` reported `no errors` for a project calling helpers that exist nowhere in the module tree; only `verify` caught it, as an AssemblyScript `Cannot find name` from inside `graph build` - no Redstart source line, and only after codegen. That is the worst place for it, because an unresolved call is the single most common thing a half-finished port has, and `check` is the fast inner loop: it is what `redstart dev` re-runs on save and what `check --json` hands an agent. A bare `name(…)` call is now resolved against every `fn` declared anywhere in the project, and a miss is an error at the call site. The fix-it is tailored: a case-only mismatch suggests the declared spelling, and an entity name called as a function points at `Entity.create(id, { … })`. Deliberately narrow to avoid false positives - only single-segment callees are judged, so `mod::helper(…)` is left alone, and every other call shape (`Entity.load`, `<Contract>.bind`, `<Template>.create`, `log.info`, `BigInt.fromI32`) was never at risk. The catalogue is 36 codes (30 errors, 6 warnings).
- **`redstart test`: the `log` namespace.** `log.debug`/`info`/`warning`/`error` evaluate their arguments and yield unit; `log.critical` fails the test, since in graph-node it terminates the handler. A handler that logged could not be tested at all before.
- **`redstart test`: the transaction fields the porting guide documents.** `.from`, `.to`, `.value` and `.gasPrice` (plus `gasLimit`, `gasUsed`, `nonce`, `index`) on the mock transaction, which carried only `.hash`.
- **`redstart test`: numeric and string surface.** `BigInt.fromI32`/`fromString`/`fromByteArray` and `BigDecimal.fromString`; `pow`, `mod`, `leftShift`, `rightShift`, `neg`, `equals`/`notEqual`/`lt`/`gt`/`le`/`ge`; `BigDecimal` arithmetic, which previously fell through to the `BigInt` path and silently failed; `startsWith`, `endsWith`, `includes`, `slice`, `charAt`, `concat`, `split`, `toLowerCase`, `toUpperCase`, and `.length` on strings and byte strings; `push` on arrays. Any subgraph doing real arithmetic was untestable natively.
- **`assertMissing(Entity, id)`.** Asserts an entity was *not* written. A test that only checks a handler did not abort asserts nothing; this is what makes it a test.

## [0.16.0] - 2026-08-10

The porting release.

### Fixed
- **Handler symbols are source-qualified when they would collide.** Two sources handling the same event name (three registry contracts all emitting `CollSurplusPoolAddressChanged`, say) emitted three identically-named exported functions, three imports sharing one alias, and three manifest entries pointing at the same symbol, which `graph build` rejects with `Duplicate identifier`. Colliding handlers now become `handle<Source><Event>` and their trigger classes are aliased per source. A name with nothing to disambiguate is untouched, so the common single-source case still reads `handleTransfer`.
- **A bound ABI is imported and listed in the manifest.** `Abi.bind(addr)` for a contract the data source doesn't itself watch, such as reading a token's `symbol()` from a factory handler or a registry from inside a helper `fn`, emitted the contract class with no import and no manifest `abis:` entry, failing eject with `Cannot find name`. Every bind, including in a helper, is now resolved across the project: the ABI is listed on each data source and imported from the module `graph codegen` writes it to. Declaring an inert template with a dummy block handler is no longer necessary.
- **Narrow Solidity integers lower correctly.** A `uint8`/`uint16`/`uint24` or `int8`…`int32` parameter was typed `BigInt`, so `event.params.op == 1` emitted `.equals(1)` on what `graph codegen` had made a native `i32`. The width table now matches graph-cli's exactly, and Solidity array types map to lists.
- **A description containing a colon no longer breaks the manifest.** It is emitted as a quoted YAML scalar. Previously the graph CLI refused to parse `subgraph.yaml` at all.
- **`Bytes.empty` / `Address.zero` are called, not referenced.** They lowered to a bare function reference that failed to compile, like `BigInt.zero` before it.

### Added
- **`redstart verify`** builds, then runs `graph codegen` + `graph build` over the output. `check` and `build` answer questions about your source; `verify` answers the one that decides a deploy, and it is the command to put in CI. (`deploy --dry-run` already did this. `verify` is the honest name for it.)
- **`entity.field = None`** clears an optional field, lowering to the generated setter's `unset`, with no raw `store.get`/`unset`/`store.set` escape hatch. `E056` rejects clearing a field that isn't `Option<T>`.
- **`Boolean` is accepted** alongside `Bool`, because that is what every `schema.graphql` says. The checker used to reject it while its own error text recommended it.
- **`E057`, unknown trigger parameter.** `event.params.x` (or `call.inputs.x`) naming something the ABI doesn't declare is now an error listing the real parameter names, instead of passing through to fail at `graph build`. Unnamed ABI inputs are `param0`, `param1`, … exactly as `graph codegen` names them.
- **`E055`, `.save()` teaches instead of confusing.** It used to report "`X` has no field `save`". It now explains that entities dirty-track and save themselves. The catalogue is 35 codes (29 errors, 6 warnings).
- **[Porting an existing subgraph](https://nightswatchhq.github.io/redstart/porting.html)**, a book chapter covering the schema and mapping translation tables, exactly what an event handler can see (and that `event.receipt` is not available), the transaction-keyed staging-entity pattern that replaces receipt inspection, multi-source event names, and multi-network deployment.
- `redstart new` writes a fuller `.gitignore`, and `redstart build` says plainly that its output is generated.

## [0.15.0] - 2026-07-10

Author-side MCP server — roadmap §5.3 (the agent keystone).

### Added
- **`redstart mcp` — a Model Context Protocol server.** Exposes the toolchain the
  compiler already owns as MCP tools an AI agent can drive in a write → check → fix
  loop, over stdio (newline-delimited JSON-RPC 2.0, no external SDK):
  - **`check`** — type-check a project (`path`) or inline `source` and return the
    same structured diagnostics as `redstart check --json` (errors *and* lint
    warnings, each with a code, message, and file/line). A parse/load failure is
    returned as a normal `{ ok: false, diagnostics }` result, not a tool error, so
    the agent always gets machine-readable feedback. This is the keystone tool.
  - **`explain`** — explain a diagnostic code (what triggers it, the footgun it
    prevents, the fix); omit `code` to list every code.
  - **`build`** — emit `schema.graphql`, `subgraph.yaml`, and `src/mappings.ts`,
    returning the artifacts (and optimisation notes); `write: true` also writes them.
  - **`test`** — run the project's `test` blocks natively and return per-test results.

  New `redstart-mcp` crate; the CLI gains the `redstart mcp` subcommand alongside
  `redstart lsp`.

## [0.14.0] - 2026-07-10

Optimising compiler — roadmap §4.3 (`fix --ids` completeness).

### Changed
- **`redstart fix --ids` now converts the via-a-local id pattern.** The most common
  real-world shape — `let id = addr.toHexString(); Entity.create(id, …)` — is now
  auto-converted (previously reported and skipped): the `.toHexString()` is dropped
  on the `let` binding, the construction site is left untouched, and the entity is
  flipped to `Id<Bytes>`. It stays conservative: the local is converted only when it
  is used **exactly once** in the handler/function body (that single id site), so
  re-typing it to `Bytes` can't affect any other reader. A local that is read again
  (logged, concatenated, used as a second id) is still reported and left untouched
  (`⤫ … id built via a local that is used more than once`).

## [0.13.0] - 2026-07-10

Optimising compiler — roadmap §4.4 (stored arrays → `@derivedFrom`).

### Added
- **W050 — stored array of entity references.** The checker now flags an entity
  field typed `[Child]` (a *stored* array of another entity, not a `derived from`
  relation) and steers to `@derivedFrom`. graph-node keeps such arrays inline and
  rewrites the whole array into a new versioned row on every append, so a growing
  one-to-many is **O(n²) on disk**; a `@derivedFrom` reverse lookup is computed on
  read and never stored. Scalar and enum arrays (`[String]`, `[BigInt]`,
  `[TokenStandard]`) are genuinely stored values and are never flagged, and the
  recommended `derived from` form stays clean. The diagnostic suggests a concrete
  back-ref field name. Registered in `redstart explain W050`. Brings the
  diagnostic catalogue to 32 codes (26 errors, 6 warnings).

  Like W040, this is a warning rather than an auto-rewrite: the migration needs a
  back-ref field the author chooses (and it changes the stored data model), so it
  can't be applied mechanically the way immutability inference can.

## [0.12.0] - 2026-07-10

Optimising compiler — roadmap §4.3 (Bytes-ids, the rewrite half).

### Added
- **`redstart fix --ids` — the W040 autofix.** Turns the `String`→`Bytes` id
  lint into an opt-in, in-place rewrite: it flips the entity's schema declaration
  to `Id<Bytes>` *and* drops the `.toHexString()` at every construction site, in
  one coordinated pass across all modules. A `Bytes` id indexes ~28% faster and
  stores ~48% less than the equivalent hex-string id (Edge & Node benchmark).

  Conservative by construction — an entity is converted only when **every** one of
  its id sites (in handlers, functions *and* `test` blocks) is provably a single
  stringified `Bytes`/`Address` value. A single literal-string id, composite key,
  or id built via an intermediate local and the whole entity is left untouched and
  reported (`⤫ Entity  skipped: …`), so the command never emits code that fails to
  check. It re-verifies the result and refuses to leave a broken tree.

  `--dry-run` previews the plan without writing. Because a `Bytes` id changes the
  stored id representation (hex-string → raw bytes), this is a deliberate data
  change — hence opt-in — so redeploy affected subgraphs from the relevant block.
  `redstart explain W040` now points at the fixer.

## [0.11.0] - 2026-07-07

Optimising compiler — roadmap §4.3 (Bytes-ids, the id half).

### Added
- **W040 — stringified-id warning.** The checker now flags an entity keyed on a
  single `Bytes`/`Address` value stringified with `.toHexString()` / `.toHex()`
  (directly, or via a local), and recommends `id: Id<Bytes>`. A `Bytes` id (with
  immutability) indexes ~28% faster and stores ~48% less than the equivalent
  hex-string id (Edge & Node benchmark). Genuine composite keys
  (`a.toHexString() + "-" + b…`) and literal-string ids are never flagged.
  This is a warning rather than an auto-rewrite: converting a String id to `Bytes`
  changes the stored id representation (hex-string → raw bytes), so it is a data
  change the author opts into — unlike immutability inference, which is
  store-identical. Registered in `redstart explain W040`. Brings the diagnostic
  catalogue to 31 codes (26 errors, 5 warnings).

## [0.10.0] - 2026-06-27

Optimising compiler — roadmap §4.5.

### Added
- **`indexerHints: prune: auto` by default.** graph-node's default is
  `prune: never` (largest DB, slowest queries); Redstart now emits `prune: auto`,
  which keeps only the history needed for reorgs — a smaller database and faster
  queries, with no effect on current-state queries. The eject path is unchanged
  (`graph build` accepts it). Reported as a build note.

## [0.9.0] - 2026-06-27

The optimising compiler begins — roadmap §4.3 (Lever 2).

### Added
- **Inferred `immutable` entities.** The checker now proves which entities are
  append-only — created but never loaded (`load`/`loadInBlock`/`loadOrCreate`) and
  never field-mutated anywhere — and codegen emits `@entity(immutable: true)` for
  them automatically. Immutable entities index faster and use far less disk
  (Edge & Node benchmark: up to 19% faster / 48% less disk). `redstart build`
  reports each inference (`⚡ inferred @entity(immutable: true) for …`).
  Conservative by construction; consistent with the gate-proven hand-annotated
  immutability on the ERC-20 fixture.
- `Generated.notes` — informational optimisation notes, separate from warnings.

## [0.8.0] - 2026-06-27

Division footguns — roadmap §3.4 / §3.5.

### Added
- **E090** — dividing by a statically-zero value (`/ 0`, `/ 0.0`,
  `/ BigInt.zero()`, `/ BigDecimal.zero()`) is now a compile error. A divide-by-
  zero is a *fatal, deterministic sync halt* (`attempted to divide … by zero`).
- **W030** — assigning a `BigInt / BigInt` result to a `BigDecimal` field is
  flagged: integer division truncates the fraction first (the canonical Uniswap
  "price returns 0" bug). Use `.divDecimal()`. Integer division into integer
  fields is unaffected (no false positives).
- `redstart explain` covers both new codes.

## [0.7.0] - 2026-06-27

The store-diff kill-gate is **GREEN**, and a deploy footgun is gone.

### Added
- **Store-diff conformance gate proven.** `conformance/fixtures/arb-erc20` (the
  ARB token on Arbitrum One) deployed to a live graph-node alongside the
  hand-written reference and produced a **byte-identical store** — 0 diffs across
  10 Account + 13 Transfer entities at block 477,660,492. Indexing fidelity, the
  roadmap's #1 risk, is no longer a hypothesis.

### Fixed
- **ABI normalisation on build.** Emitted ABIs now always carry `anonymous` on
  event entries. graph-node *requires* it at deploy time (even though `graph
  build` doesn't), so an ABI missing it caused a cryptic `graph deploy` failure.
  Redstart now adds it automatically.

## [0.6.0] - 2026-06-27

Performance lint: the "stuck at 3%" eth_call — roadmap §4.2.

### Added
- **W020** — a contract call (`eth_call`) inside a `for`/`while` loop is now
  flagged. Each call is a 100 ms+ blocking RPC run serially while the handler is
  paused, so N iterations means N round-trips — the classic "stuck at 3%" sync.
  Calls outside loops are unaffected (no false positives). `redstart explain
  W020` documents it.

## [0.5.0] - 2026-06-26

Handler-shape lints + warning-severity diagnostics — roadmap §4.7.

### Added
- **Warning-severity diagnostics.** Diagnostics can now be warnings (lints):
  they're reported but don't fail the build. `check --json` carries the real
  `severity`; `redstart check` prints warnings and still exits 0.
- **W010** — a `handler call` on a network without Parity call tracing
  (Arbitrum, Optimism, Base, Polygon, BNB, …), where the handler silently never
  fires. Prefer an event handler.
- **W011** — an unfiltered block handler (runs on every block of the whole
  chain). Add `every N` or `once`.
- `redstart explain` covers both new codes.

Determinism by construction — roadmap §3.7.

### Added
- **E080** — calling a non-deterministic host function (`Date.now`, `Date.UTC`,
  `Date.parse`, `Math.random`) is now a compile error, with a fix-it pointing at
  `event.block.timestamp`. Non-determinism diverges Proof-of-Indexing across
  indexers (a slashing risk); graph-node only blocks some of these at runtime.
  `redstart explain E080` documents it.

## [0.3.0] - 2026-06-26

Errors that teach — the second roadmap step (§5.5).

### Added
- `redstart explain <CODE>` — explains any diagnostic code: title, what
  triggered it, **the bug it prevents**, and the canonical fix. Supports
  `--json`; bare `redstart explain` lists all 24 codes.
- A diagnostic-code registry in `redstart-checker` (`explain` module), the
  shared source of truth for code explanations.

## [0.2.0] - 2026-06-26

First step of the [2026 roadmap](docs/ROADMAP-2026.md) — agent-native diagnostics.

### Added
- `redstart check --json` — machine-readable diagnostics for editors and agent
  loops: `{ "ok": bool, "diagnostics": [ {code, severity, message, label, help,
  file, line, column, offset, length} ] }` on stdout, non-zero exit when not ok.
  Lex/parse/resolution failures are emitted too (ANSI stripped). (Roadmap §5.1.)
- `Diag` now carries 1-indexed `line`/`column` and exposes `code_short()`
  (e.g. `E062`) and `label_str()`.

## [0.1.0] - 2026-06-26

Stage 0 — foundations. The first end-to-end vertical slice.

### Added
- Lexer + recursive-descent parser with `miette` diagnostics (`redstart-parser`).
- `redstart.toml` manifest + multi-file module tree with cycle detection (`redstart-loader`).
- Semantic checker: nullable-deref, derived back-refs, required-field init,
  arithmetic-on-`Option`, `.value`-without-`match` (`redstart-checker`).
- AssemblyScript lowering + `schema.graphql` / `subgraph.yaml` generation,
  eject-verified through canonical `graph build` to WASM (`redstart-codegen`).
- Event, call, block, and file handlers; dynamic data sources (templates);
  timeseries entities + aggregations; enums, interfaces, derived fields.
- Native test interpreter — mock store and call mocking, no WASM/Docker
  (`redstart-test`).
- CLI: `new`, `check`, `build`, `test`, `fmt`, `dev`, `deploy` (`redstart-cli`).
- Language server with diagnostics, formatting, hover, go-to-def, completion
  (`redstart-lsp`); tree-sitter grammar + VS Code extension.
- Conformance harness: field-level store-diff against reference subgraphs.
- Release pipeline: cross-compiled binaries (macOS arm64/x86_64,
  Linux x86_64/arm64) to GitHub Releases + Homebrew tap.

[Unreleased]: https://github.com/nightswatchhq/redstart/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/nightswatchhq/redstart/releases/tag/v0.1.0
