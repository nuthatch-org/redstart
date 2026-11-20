# Generator regression harness

Measures the "≥80% first-try green" benchmark and catches regressions as the
generation prompt evolves. For each contract in [`corpus.ts`](./corpus.ts) it:

1. hits the deployed `/api/contract` (the **real** research pipeline: proxy
   resolution, ABI, start block),
2. runs the **real** generation (`lib/generate.ts`, the exact prompt users get),
3. runs genuine `graph codegen && graph build && graph test` against the output.

No stubs. It's the same toolchain the compile gate runs, plus Matchstick tests.

## Setup (once)

```bash
cd scripts/harness/base && npm install   # graph-cli + graph-ts + matchstick-as
```

## Run

```bash
# Whole corpus against production. Needs a TEST-ONLY key (no human to BYOK here).
ANTHROPIC_API_KEY=sk-ant-... npx tsx scripts/harness/run.ts

# One contract, verbose errors:
ONLY=Seaport VERBOSE=1 ANTHROPIC_API_KEY=sk-ant-... npx tsx scripts/harness/run.ts

# Against local dev instead of production:
HARNESS_API=http://localhost:3000 ANTHROPIC_API_KEY=sk-ant-... npx tsx scripts/harness/run.ts
```

Output: a per-contract scorecard, the first-try-green %, any results below the
corpus target, and a frozen `results.json`.

## Notes

- The **test key is a legitimate testing cost**, separate from the $0 BYOK user
  model, since there's no human pasting a key in an automated run.
- Tier 4 (factories) currently produce a *flat* subgraph; factory templates
  aren't built yet, so those entries measure "does the flat output compile," not
  "are the children indexed." That gap is the spec for the next build.
