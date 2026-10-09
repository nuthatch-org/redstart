# RFC-0001: Unified source of truth & the eject path

- Status: Accepted (foundational — documents the design Redstart is built on)
- Author(s): The Lodestar Team
- Created: 2026-06-26
- Tracking issue / PR: N/A (retroactive record of the founding design)

## Summary

Redstart unifies the three artifacts of a subgraph — `schema.graphql`,
`subgraph.yaml`, and the AssemblyScript mappings — into a single, type-checked,
multi-module language that transpiles to readable AssemblyScript the canonical
`graph build` toolchain compiles unmodified. This RFC records *why* that shape was
chosen, since every later decision is measured against it.

## Motivation

A subgraph today is three loosely-coupled files held together by stringly-typed
names and a manual `graph codegen` step. The dominant failure mode is **drift**:
an event renamed in the ABI but not the manifest, a schema field the mapping
forgets to set, a `.save()` left off. These compile fine and fail at runtime,
often hours into a sync.

Separately, hand-written AssemblyScript mappings carry a catalogue of footguns —
nullable-arithmetic miscompiles, `==`/`===` inversion, reverted calls aborting the
handler, array prefill, forgotten saves.

Both problems share a root cause: there is no single artifact that the toolchain
type-checks end to end. So we build one.

## Guide-level explanation

You write `.red` modules declaring entities, ABIs, sources, handlers, helpers, and
tests. The compiler resolves names across every module and checks them against one
another. `redstart build` projects the unified AST into the three canonical files.
See [the book](https://github.com/nuthatch-org/redstart) for the language tour.

## Reference-level explanation

- **Single AST, multiple projections.** Schema, manifest, and mappings are all
  *projections* of one checked AST. Drift is impossible because there is nothing
  to drift *from* — they cannot disagree.
- **Derivation by reference.** Event signatures in the manifest are derived from
  the imported ABI, so a rename is a compile error.
- **Footguns made unrepresentable.** Nullability is `Option<T>` with no `null`;
  contract calls return `Result` and must be `match`ed; entities are dirty-tracked
  and auto-saved; derived fields are read-only. Each footgun becomes a parse/type
  error.
- **The eject path is non-negotiable.** Output must be readable, idiomatic
  AssemblyScript that `graph codegen` + `graph build` accept unmodified. This is
  what defuses the bus-factor objection: abandoning Redstart costs only the
  single-source-of-truth convenience; the generated code keeps working.

## Drawbacks

- A new language is a real adoption cost and a maintenance burden.
- The lowering must track graph-ts and graph-node semantics precisely; the
  conformance store-diff is the gate that keeps us honest, and it is load-bearing.

## Alternatives

- **A linter / codegen layer over AssemblyScript.** Rejected: it can't unify the
  manifest and schema, and can't make footguns unrepresentable — only flag them.
- **A typed DSL embedded in TypeScript.** Rejected: the host language's footguns
  leak through, defeating the point.

## Unresolved questions

- The precise boundary of graph-ts surface coverage we commit to supporting.
- How far the native test interpreter should model graph-node before deferring to
  a real deployment in conformance.
