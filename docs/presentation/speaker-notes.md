# Redstart intro: speaker notes

Companion script for [`redstart-intro.html`](./redstart-intro.html). Open the deck,
present full-screen, use `←` / `→` (or space) to advance. 15 slides.

- **Runtime:** ~18-22 minutes at a comfortable pace, or ~10 if you cut the
  graveyard detail and the Rust slide.
- **Audience:** subgraph developers, indexers, and Graph ecosystem people. They
  know what a subgraph is and have felt AssemblyScript bite them. You do not need
  to explain The Graph from scratch.
- **Voice:** first person. The story hinges on you being the person who built
  Matchstick and tried four times to get off AssemblyScript. Lean into that. The
  credibility of the whole talk comes from "I have paid for this knowledge."
- **The one thing to land:** AssemblyScript is a *language*; Redstart is a
  *compiler that knows it is building a subgraph*. Everything else is evidence for
  that sentence.

Delivery tips:

- The deck is dense on purpose so it reads well as a leave-behind. On stage, do
  not read it. Say the idea, point at the evidence, move on.
- Slides 3, 8, and 9 are the proof slides. Slow down there. Everything before is
  setup; everything after is payoff.
- Numbers are your friends because they are checkable. "0 diffs at block
  477,660,492" is more persuasive than "it works well."
- If you are short on time, the spine is slides 0 → 1 → 3 → 5 → 8 → 9 → 14.

---

## Slide 0: Title

**On screen:** the Redstart wordmark, "A language for authoring The Graph
subgraphs," and the thesis line about AssemblyScript vs a DSL that knows it is
building a subgraph.

**Talk track:**

> This is Redstart. It's a language for writing subgraphs on The Graph.
>
> Here's the whole idea in one line, and everything after this is just evidence
> for it. AssemblyScript is a general-purpose language, and not a great one.
> Redstart is a small language whose compiler knows exactly one thing: that it is
> building a subgraph. That single fact is the entire advantage, and it's the
> reason a compiler can do things a library never can.

**Beat:** pause on the thesis for a second before moving on. Let it sit.

**Transition:** "So let me show you why writing a subgraph today is harder than it
should be."

*~1 min.*

---

## Slide 1: The problem: three files held together by string matching

**On screen:** `schema.graphql`, `subgraph.yaml`, and `mappings.ts` side by side,
then the paragraph about what `graph build` does and does not catch.

**Talk track:**

> A subgraph is three files. A schema that describes your data, a manifest that
> wires events to handlers, and AssemblyScript mappings that do the indexing. They
> are three separate artifacts, and the only thing holding them together is
> matching strings and a manual codegen step.
>
> When you run `graph build`, it runs the AssemblyScript compiler, so type errors
> against the generated classes get caught. Good. But the rest doesn't. Forget a
> `.save()` and the entity just never persists. Let a handler's signature drift
> from what's on chain and it silently never fires. Leave a required field unset
> and it fails at deploy, not at build.
>
> The failure you remember from AssemblyScript is almost never the compile error.
> It's the green build that dies three hours into a sync, on a field you had
> stopped thinking about.

**Key point:** the enemy is not "AssemblyScript is hard." The enemy is *late*
failure. Things that compile clean and break in production.

**Transition:** "And part of why it fails late is the language itself, so let me
be specific about that."

*~1.5 min.*

---

## Slide 2: AssemblyScript looks like TypeScript, it isn't

**On screen:** the nullability code example (a nullable local is caught, the same
sum on a nullable property compiles clean), plus three bullets on the standard
library and drifting semantics.

**Talk track:**

> AssemblyScript looks like TypeScript, and that's the trap. It's a TypeScript-ish
> subset compiled to WebAssembly, with its own compiler and its own garbage
> collector. Because the syntax is familiar, you bring your TypeScript instincts,
> and they are wrong in exactly the places that cost you.
>
> Here's the sharpest example, and it's from The Graph's own migration guide.
> AssemblyScript narrows nullability for local variables but not for property
> accesses. So these two identical-looking sums compile differently: the local
> one is a caught error, the property one compiles clean and crashes at runtime.
> The compiler won't add the null check, so the docs tell you to write it by hand.
>
> On top of that: a bare standard library with no `Date` type, so you hand-roll
> timestamp maths. And semantics that have shifted between releases. Exponentiation
> quietly changed from float to integer results. Variable shadowing was removed.
> Nullability behaviour changed version to version.

**Beat / slide closer:** "The failures that matter compile clean and only surface
at runtime. That's the problem."

**Transition:** "So the obvious answer is a linter. And a linter helps. But it
can't be enough, and here's why."

*~1.5 min.*

---

## Slide 3: Why a linter isn't enough (the footgun table)

**On screen:** the table mapping diagnostic codes to the bug each makes
impossible, then the eight named Subgraph Linter checks as pills, then the "30
codes" line. **This is a proof slide. Slow down.**

**Talk track:**

> A linter can only warn you about a bug it understands. And AssemblyScript, the
> language, has no concept of an entity, a `.save()`, a derived field, or a
> reverted `eth_call`. So entire classes of subgraph bug are just invisible to it.
>
> Redstart makes them unrepresentable. Not a warning you can suppress, but a
> compile error, or a shape that isn't even in the grammar. Every row here is a
> real, documented failure mode. A null dereference from a `load` that returned
> nothing. Arithmetic on a nullable that compiles silently and crashes. A reverted
> call that aborts your handler and diverges Proof-of-Indexing. A divide-by-zero
> that halts the whole sync. `Date.now` and `Math.random`, which are
> non-deterministic and can get an indexer slashed. The canonical Uniswap
> price-equals-zero bug. The "stuck at 3 percent" eth-call-in-a-loop.
>
> And this isn't me inventing a wishlist. The Graph already ships a Subgraph
> Linter that names these eight checks. Its own docs describe them as things that
> compile fine and then crash at runtime. A linter is a separate pass you can skip,
> and the code ships anyway. Redstart turns the same footguns into compile errors
> or absent grammar. Right now that's 31 codes, and counting.

**Key point:** frame the linter as *validation of the thesis by The Graph itself*.
They admit these are footguns. Redstart's move is to make them unrepresentable
rather than lintable.

**If asked "why 31 and not the 8?":** the linter's 8 are the named ones; Redstart
also covers determinism, precision, division, handler shape, and structural checks
the linter doesn't have.

**Transition:** "Now, why am *I* the one telling you this? Because I've spent four
years trying and failing to solve it a different way."

*~2.5 min.*

---

## Slide 4: The graveyard: four failed escapes

**On screen:** the epigraph about building Matchstick in 2021, then the ledger of
four projects: yogurt, a native ABI, Graphite, liminal.

**Talk track (personal, this is the emotional center):**

> I built Matchstick in 2021. It's the testing framework you run when you type
> `graph test`. Building it meant learning AssemblyScript's memory model by heart,
> which is exactly the monster that would eat every attempt I made to leave it.
> I've been trying to get off AssemblyScript ever since. Four times.
>
> First, yogurt: fake it from the outside. Pure Rust, emulating the AssemblyScript
> memory model at the binary level. The output side worked, but reading event
> parameters back out of graph-node's AssemblyScript-formatted memory was
> unfakeable from where I was standing. I archived it after 66 deploy iterations.
>
> Second, a native ABI: change graph-node itself. A parallel Rust ABI, about 1,450
> lines, benchmarked around 617,000 Transfer events a second. I opened PR 6462. The
> maintainers said no, and they were right. A second runtime is a permanent second
> surface to maintain. The maintenance burden *was* the product.
>
> Third, Graphite: become AssemblyScript. I reimplemented the AS runtime in Rust
> and shipped ERC-20 and ERC-721 live on Arbitrum One. But now I owned a
> byte-for-byte runtime forgery that has to stay in lockstep with the AssemblyScript
> compiler, forever.
>
> Fourth, liminal: leave subgraphs entirely. A WASIp2 component runtime, a third
> lane next to Subgraphs and Substreams. Real, but not a subgraph. No
> Proof-of-Indexing, no GraphQL on the network. That's escape by emigration.

**Slide closer:** "Every one of them either fought the runtime or fled it. And the
runtime was never the enemy."

**Key point:** this slide earns the rest of the talk. It proves you tried the
obvious things so the audience doesn't have to suggest them. Deliver it honestly,
not as a highlight reel. The failures are the credential.

**Transition:** "So if the runtime was never the enemy, what was?"

*~2.5 min.*

---

## Slide 5: The turn: stop fighting the runtime

**On screen:** four crossed-out strategies (each mapped to a graveyard project),
the statement "Replace the part you actually hate: writing it," and the technical
note about what graph-node actually loads.

**Talk track:**

> Here's the realization. Don't fake it from outside, that was yogurt. Don't patch
> the protocol, that was the native ABI. Don't reimplement the runtime, that was
> Graphite. Don't flee the paradigm, that was liminal.
>
> Replace the one part you actually hate: writing the AssemblyScript by hand.
>
> The AssemblyScript compiler is fine. It's battle-tested, it's maintained by
> people who aren't me, and it's the canonical path the whole network already
> trusts. What hurts is hand-writing AssemblyScript across three files that don't
> check each other.
>
> And here's the key technical fact. graph-node doesn't load a *language*. It
> checks the manifest says `wasm/assemblyscript`, it reads two numbers off each
> object's header, and it matches host functions by name. A memory layout, an
> allocator protocol, a string encoding. That's all it speaks. So let the official
> compiler emit that, and replace only the writing.

**Key point:** this is the pivot of the whole talk. The insight is that you can
sit *above* AssemblyScript instead of fighting or replacing it.

**Transition:** "That's a compiler, not a library, and the difference matters more
than it sounds."

*~1.5 min.*

---

## Slide 6: Why a compiler, not a library

**On screen:** the ABI-coupling tech note (rtId / rtSize, host functions by name),
then two cards contrasting graph-ts (a library inside AssemblyScript) with Redstart
(a compiler above it).

**Talk track:**

> Why does it have to be a compiler? Because graph-node doesn't run WASM in general.
> It runs AssemblyScript-shaped WASM. Compile Rust or Go or C to `wasm32` and
> graph-node can't read a single entity out of it, because it's looking for
> AssemblyScript's object headers and resolving host functions by exact name. The
> WASM target is fungible. The ABI is not. That's why you can't just bring another
> language, and it's why the fix has to *produce* AssemblyScript rather than replace
> it.
>
> Now, `graph-ts` is a library, and it lives *inside* AssemblyScript. A library
> can't out-think the language it lives in. It hands you types and bindings and
> then stops. The sharp edges are still yours. A forgotten `.save()` is still a
> silent bug. It can't infer immutability, it can't optimise the output, and it
> can't prove any of it.
>
> A compiler has its own front-end, and AssemblyScript becomes just the backend.
> Footguns become ungrammatical. Schema, manifest, and handlers are one checked
> program. And it can do whole-program analysis, and prove the result is
> byte-identical.

**Slide closer:** "A library inherits AssemblyScript's limits. A compiler makes
AssemblyScript an implementation detail."

*~2 min.*

---

## Slide 7: What it does: one language, one source of truth

**On screen:** the pitch bullets on the left (event signature by reference, no
`null`, auto-save) and a full `main.red` example on the right.

**Talk track:**

> So here's what it actually is. Redstart folds all three artifacts into one
> language, split across as many `.red` modules as you like, with `mod` and `use`
> just like Rust. And it type-checks them against each other. Your entities can
> live in one module and the handlers that write them in another, and the compiler
> resolves and checks across all of them.
>
> A few things fall out of that. The event signature in the manifest is derived
> from the ABI by reference, so if you rename the event, that's a compile error,
> not a handler that silently never fires. There is no `null`; nullability is
> always explicit as `Option`. And entities are dirty-tracked and auto-saved, so
> forgetting `.save()` can't happen.

**Point at the code:** "This one file is the ABI import, the entity, the source,
and the handler. One source of truth. Nothing to drift."

**Transition:** "And the output it produces is the interesting part."

*~1.5 min.*

---

## Slide 8: How: the transpile (the hero slide)

**On screen:** `transfer.red` on the left, the emitted `mappings.ts` on the right,
and four transform cards underneath. **This is a proof slide. Slow down.**

**Talk track:**

> This is the core of it. On the left is what you write. On the right is the
> AssemblyScript it emits. And the point is that the output is exactly what a
> careful human would have written by hand. It's readable and idiomatic. It is not
> some opaque intermediate.
>
> Look at the transforms. `loadOrCreate` becomes a `load`, a null guard, and an
> init, so no raw nullable dereference ever survives. The arithmetic lowers to the
> explicit graph-ts form, and more importantly, doing maths on an `Option`
> upstream is a compile error, where AssemblyScript would have compiled it silently
> and crashed. The auto-save becomes real `.save()` calls, flushed at handler end,
> even across a helper's early return. And a `match` on a call result becomes the
> `try_` form with the reverted check, so a reverted `eth_call` is always handled.

**Key point:** the emphasis is *readability of the output*. This is what defuses
the "black box compiler" fear. What comes out is code you'd sign off on in review.

**Transition:** "But readable isn't the same as correct. So how do we know it's
correct?"

*~2 min.*

---

## Slide 9: How: the eject path and the kill-gate

**On screen:** the four-step pipeline, two cards (the eject path, the kill-gate),
and the green "GATE GREEN" banner with the store-diff result. **Proof slide.**

**Talk track:**

> Two things make this safe to bet on.
>
> First, the eject path. The AssemblyScript we emit goes straight through the
> official toolchain: `redstart build`, then `graph codegen`, `graph build`,
> `graph deploy`. Unmodified. So if you ever walk away from Redstart, you keep the
> generated code and it keeps working with the standard tools. The cost of leaving
> is zero, and that defuses the usual objection to betting production infra on a
> young language.
>
> Second, and this is the one I care about most: we don't just claim fidelity, we
> continuously try to disprove it. There's a kill-gate. We deploy the generated
> subgraph next to an independently hand-written reference, on a live graph-node,
> and we diff the two stores at a fixed block. If they aren't identical, the whole
> approach is wrong.
>
> And it's green. The ARB token on Arbitrum One produced a byte-identical store.
> Zero differences, across every Account and Transfer entity, at a fixed block.
> Indexing fidelity is no longer a hypothesis. It's a checked fact.

**Key point:** the kill-gate is the intellectual honesty of the project. You built
a way to prove yourself wrong, and it keeps passing. Say that plainly.

**Transition:** "So that's the how. Let me tell you why two different audiences
care: developers, then indexers."

*~2 min.*

---

## Slide 10: Why developers care

**On screen:** three cards (errors that teach, no drift, native testing), a
`tests.red` example, and the one-binary toolchain pills.

**Talk track:**

> For the person writing the subgraph, three things. Errors that teach:
> `redstart explain` gives you the bug a diagnostic prevents and the canonical fix,
> not just "type error on line 12." Errors are treated as the product. No drift:
> schema, manifest, and mappings are all projections of your `.red` source, so they
> can never disagree, because you never hand-write them separately. And native
> testing: tests run in a tree-walking interpreter over a mock store. No WASM
> compile, no Docker, no Matchstick binary, no version skew between
> `matchstick-as` and `graph-ts`.
>
> And it's all one static binary. Language server, formatter, a watch mode, a
> tree-sitter grammar for Neovim, Helix, and Zed, a VS Code extension, and a
> browser playground that runs the real compiler.

**Point at the test:** "You fire an event at your handlers and assert on the
resulting store, and you can mock an `eth_call` inline. Sub-second, no
infrastructure."

*~1.5 min.*

---

## Slide 11: Why indexers care

**On screen:** the "leaner subgraph, applied for you" pitch, two shipping
optimisations (inferred immutable, prune: auto), the W020 lint, and the closed-loop
gate banner.

**Talk track:**

> For indexers, the pitch is different. A human writes some AssemblyScript.
> Redstart writes every line of it, from a model that owns the schema, the
> manifest, and the mappings. So it applies the storage best-practices that most
> subgraphs never get around to, uniformly, every time. This won't beat perfectly
> hand-tuned AssemblyScript. It's a floor no one forgets to raise.
>
> Two of these ship today. Inferred immutability: the checker proves which entities
> are append-only, created and never mutated anywhere, and marks them immutable for
> you. On Edge and Node's benchmark that's up to 19 percent faster indexing and 48
> percent less disk. And pruning: graph-node's own default is to keep the entire
> history forever, a bigger database and slower queries. Redstart emits
> `prune: auto` by default, keeping only what serving needs.
>
> And the "stuck at 3 percent" lint catches an eth-call inside a loop before you
> deploy. That one warns today; turning it into an automatic rewrite is next.
>
> Here's the part that makes all of this possible. Because the store-diff gate is
> green, we can optimise the emitted code with a safety net: change the output,
> re-run the diff, and ship only if the store stays byte-identical. That's
> performance work that can't silently corrupt your data, which nothing else in the
> ecosystem can offer.

**Key point:** the closed loop is the differentiator. Optimise fearlessly because
the gate proves the data didn't change. Land that.

*~2 min.*

---

## Slide 12: Looking ahead: safer is nearly done, faster is the frontier

**On screen:** two cards, the optimising-compiler roadmap for indexers and the
remaining developer wins.

**Talk track:**

> Where this goes next. Safer is nearly done. Faster is the frontier, and it's the
> lever neither a language nor a library can pull.
>
> Because Redstart owns the codegen, a performance optimisation is just a better
> lowering of the same `.red`, and the kill-gate proves each one byte-identical.
> On the indexer side that's auto-filling `startBlock` from the deployment block so
> you stop scanning dead chains, declaring `eth_calls` for parallel cached
> pre-fetch, `Bytes` ids on top of inferred immutability for the benchmark's full
> gains, rewriting growing arrays to `@derivedFrom`, and coalescing redundant loads.
>
> On the developer side the last footguns fall: entity-ID collisions,
> event-signature drift, full determinism. Plus `redstart new --from` an address to
> scaffold a whole subgraph in one command, and an MCP server so an AI can author
> and fix subgraphs in the loop.

**Slide closer:** "Same network, same byte-identical data, just faster, safer, and
finally pleasant to write."

*~1.5 min.*

---

## Slide 13: Why Rust

**On screen:** the `redstart check --cost` roadmap mockup and the Rust-platform
bullets. *This slide is skippable if you're short on time.*

**Talk track:**

> One more thing about the foundation. Redstart is a real compiler written in Rust,
> with a full semantic model of your subgraph and the whole crates.io ecosystem
> behind it, none of which is ever coming to AssemblyScript. That's how it catches
> bugs, and it's also headroom.
>
> Here's a concrete example of the headroom. The network's biggest blind spot is
> that nobody knows how large a subgraph will get, or how long it'll take to sync,
> until they're already deep into indexing it. Redstart already builds the model
> that could answer that: every entity, every handler, every write. Turning it into
> a cost report you read *before* you sync is exactly the kind of thing only a
> compiler can do.

**Key point:** Rust isn't a vanity choice. It buys fuzzing, property tests,
incremental analysis, a real language server, and a single static binary with no
Node, Docker, or Postgres.

*~1.5 min.*

---

## Slide 14: Status and call to action

**On screen:** "Stage 0 is done," the three stat tiles (12.7k lines / 8 crates, 30
codes, 0 edits to eject), the install commands, and the links.

**Talk track:**

> Where it stands. Stage 0 is done. Early, but real and end-to-end. It's proven by
> porting a real subgraph, PaulieB14's Graph Horizon indexer, three contracts on
> Arbitrum One, and deploying it live to Subgraph Studio.
>
> It's scoped as a Graph-Foundation-grant public good, in the lineage of
> Matchstick, the framework this whole story started with. Not a venture bet. And
> its purpose was never to make subgraphs faster to write. It's to make *staying*
> on the decentralized network pleasant, after years of watching people reach for a
> centralized alternative the moment AssemblyScript bit them.
>
> About 12,600 lines of Rust across 8 crates, 31 teaching diagnostic codes, and
> zero edits needed to eject to the standard toolchain. You can `brew install` it
> today, and the playground runs the real compiler in your browser.

**Close:** "That's Redstart. Thank you." Then take questions.

*~1.5 min.*

---

## Appendix A: verified fact sheet

Everything below is checked against the repository and the live site as of the
v0.10.0 release. Use these numbers with confidence.

| Claim | Value |
|---|---|
| Diagnostic codes | 31 total (26 errors `E***`, 5 warnings `W***`) |
| Rust source | ~12,600 lines across 8 crates |
| Crates | parser, loader, checker, codegen, test, lsp, cli, wasm |
| Releases shipped | v0.1.0 → v0.10.0 (10 tagged releases) |
| Kill-gate fixture | `conformance/fixtures/arb-erc20` (ARB token, Arbitrum One) |
| Store-diff result | 0 diffs, 10 Account + 13 Transfer entities, block 477,660,492 |
| Immutable inference | up to ~19% faster indexing / ~48% less disk (Edge & Node) |
| graph-node prune default | `prune: never`; Redstart emits `prune: auto` |
| Live site | https://redstart-lang.com |
| Source | https://github.com/nuthatch-org/redstart |

The diagnostic codes referenced on slide 3: E062 (nullable deref), E061
(arithmetic on `Option`), E060 (`.value` of an unmatched call), E051 (incomplete
initializer), E090 (division by zero), E080 (non-deterministic call), W030 (BigInt
division precision), W020 (eth_call in a loop), W040 (stringified Bytes id).

The eight Subgraph Linter checks (slide 3): entity-overwrite, unexpected-null,
unchecked-load, unchecked-nonnull, division-guard, derived-field-guard,
helper-return-contract, undeclared-eth-call.

---

## Appendix B: the graveyard, for questions

If someone digs into slide 4, the details:

- **Matchstick (2021):** the subgraph testing framework behind `graph test`.
  Building it is how you learned the AssemblyScript memory model.
- **yogurt:** pure Rust emulating the AS memory model at the binary level. Writing
  AS-formatted memory worked; reading event params back out from the guest side did
  not. Archived after 66 deploy iterations.
- **native ABI (PR #6462):** a parallel Rust ABI, ~1,450 lines, ~617k Transfer
  events/sec. Declined by the maintainers, correctly, because a second runtime is a
  permanent second maintenance surface.
- **Graphite:** a Rust reimplementation of the AS runtime; shipped ERC-20 and
  ERC-721 live on Arbitrum One. The problem: it's a byte-for-byte runtime forgery
  that must track `asc` forever.
- **liminal:** a WASIp2 component runtime, a third execution lane beside Subgraphs
  and Substreams. Not a subgraph: no Proof-of-Indexing, no GraphQL on the network.

The through-line: three of the four fought the runtime, one fled it. Redstart does
neither. It keeps the runtime and the official compiler, and replaces only the act
of writing the AssemblyScript.

---

## Appendix C: likely questions and honest answers

**"Isn't this just another abstraction that leaks?"**
The eject path is the answer. The output is idiomatic AssemblyScript the standard
toolchain compiles unmodified, and the store-diff proves it indexes identically. If
Redstart ever fails you, you keep working code. The cost of leaving is zero.

**"Why should I trust the generated code indexes correctly?"**
That's the kill-gate. We deploy our output beside a hand-written reference on a live
graph-node and diff the stores at a fixed block. It's green: 0 diffs on the ARB
token at block 477,660,492. It runs in CI whenever an archive RPC is configured.

**"Does it support my subgraph's features?"** (templates, file data sources,
aggregations, call/block handlers)
Yes. All handler kinds (event, call, block, file/IPFS), dynamic data sources with
context, enums, interfaces, timeseries and aggregations, and the graph-ts surface.
The whole feature set ejects to WASM unmodified.

**"What about performance versus hand-tuned AssemblyScript?"**
Redstart isn't trying to beat a perfectly hand-tuned subgraph. It raises the floor:
immutability inference and `prune: auto` today, more optimisations on the roadmap,
each one proven byte-identical by the gate. Most subgraphs never apply these by
hand.

**"Who maintains it, and what if you get hit by a bus?"**
It's scoped as a Graph-Foundation-grant public good, MIT licensed, in the lineage of
Matchstick. And the eject path means the bus factor doesn't strand you: your
generated code keeps working with the canonical tools regardless.

**"Why a new language instead of improving graph-ts?"**
A library lives inside AssemblyScript and inherits its limits: no way to remove
`null`, no whole-program analysis, no way to prove the output. A compiler sits above
the language, so it can make footguns ungrammatical and optimise the emitted code.

**"Is it production-ready?"**
It's Stage 0: real and end-to-end, with a real subgraph ported and deployed live to
Subgraph Studio, but early. Be honest about that. The kill-gate being green is the
strongest claim you can make, so make that one.
