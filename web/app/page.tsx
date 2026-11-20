import Link from "next/link";
import { Reveal } from "@/components/reveal";
import { CodeBlock } from "@/components/code";
import { FileTabs } from "@/components/file-tabs";
import { Bird } from "@/components/logo";
import {
  REDSTART_ERC20,
  AS_MAPPINGS,
  AS_SCHEMA,
  AS_MANIFEST,
} from "@/lib/samples";

const REPO = "https://github.com/nuthatch-org/redstart";
const DOCS = "https://nuthatch-org.github.io/redstart/";

// Each figure is from the repo: README kill-gate and eject-path results, and
// the explanation table behind `redstart explain` (crates/redstart-checker/src/explain.rs).
const PROOF = [
  {
    n: "0",
    unit: "differences",
    d: "Store-diffed against a hand-written reference on a live graph-node: 10 Account and 13 Transfer entities at Arbitrum One block 477,660,492.",
  },
  {
    n: "0",
    unit: "manual edits",
    d: "graph codegen and graph build compile the emitted subgraph to WebAssembly unmodified.",
  },
  {
    n: "37",
    unit: "diagnostic codes",
    d: "Each one explained by redstart explain: what tripped it, and the fix.",
  },
];

const DOES = [
  "Unify schema, manifest and mappings in one typed source, across as many .red modules as you like.",
  "Emit readable AssemblyScript that graph build compiles unmodified.",
  "Handle events, calls, blocks and files, with templates and dynamic data sources.",
  "Test handlers natively with redstart test: no WASM, Docker or Matchstick.",
  "Ship a language server, with VS Code and Zed extensions.",
  "Deploy to Subgraph Studio or a self-hosted graph-node.",
];

const DOESNT = [
  "Make indexing faster. It makes staying on The Graph pleasant.",
  "Expose event.receipt, so a handler cannot read neighbouring logs.",
  "Run call handlers where there is no Parity-style tracing, which is most L2s. W010 warns.",
  "Deploy one manifest to several networks. That stays in your own configuration.",
  "Parse a ternary or a module-level constant. Use an if, or a helper fn.",
  "Emit fulltext search yet.",
];

const FOOTGUNS = [
  {
    t: "Forgotten .save()",
    d: "Entities are dirty-tracked and flushed at handler end. The most common subgraph bug isn't expressible.",
  },
  {
    t: "Arithmetic on null",
    d: "There is no null, only Option<T>. Maths on a maybe-absent value is a compile error, not a silent miscompile.",
  },
  {
    t: "Reverted calls aborting",
    d: "Contract calls return Result. You must match before touching the value, so a revert can't kill the handler.",
  },
  {
    t: "Manifest / ABI drift",
    d: "Event signatures are derived from the ABI by reference. Rename an event and it's a compile error.",
  },
  {
    t: "Writing derived fields",
    d: "@derivedFrom fields are read-only by construction. Assigning to one doesn't type-check.",
  },
  {
    t: "== vs === inversion",
    d: "One equality, lowered correctly every time. The classic AssemblyScript footgun is gone.",
  },
  {
    t: "Non-determinism",
    d: "Date.now() and Math.random() are compile errors. They diverge Proof-of-Indexing across indexers and get them slashed.",
  },
  {
    t: "Division by zero",
    d: "A statically zero denominator is a compile error. graph-node would halt the sync on it, deterministically, every time.",
  },
  {
    t: "eth_call in a loop",
    d: "A contract call inside a loop, the documented 'stuck at 3%' sync killer, is flagged before you deploy it.",
  },
];

export default function Home() {
  return (
    <>
      {/* ---- Hero ---- */}
      <section className="relative overflow-hidden border-b border-line">
        <div className="relative mx-auto max-w-6xl px-5 pb-16 pt-16 sm:pb-24 sm:pt-28">
          <Reveal>
            <span className="tag">
              <Bird className="h-3.5 w-3.5" />
              one typed language for The Graph subgraphs
            </span>
          </Reveal>
          <Reveal i={1}>
            <h1 className="display mt-6 max-w-3xl text-4xl sm:text-6xl md:text-7xl">
              Write the subgraph once.
              <br />
              <span className="text-accent">Properly.</span>
            </h1>
          </Reveal>
          <Reveal i={2}>
            <p className="mt-6 max-w-xl text-lg leading-relaxed text-muted">
              One typed source for schema, manifest and mappings, compiled to
              readable AssemblyScript that{" "}
              <code className="font-mono text-text">graph build</code> compiles
              unmodified. <span className="text-text">If it compiles, it works.</span>
            </p>
          </Reveal>
          <Reveal i={3}>
            <div className="mt-8 flex flex-wrap items-center gap-3">
              <Link href="/playground" className="btn">
                Open the playground
                <span aria-hidden>→</span>
              </Link>
              <a href={REPO} className="btn btn-ghost">
                View on GitHub
              </a>
            </div>
          </Reveal>
          <Reveal i={4}>
            <div className="mt-6 flex w-full max-w-full items-start gap-3 rounded-lg border border-line bg-inset px-4 py-2.5 font-mono text-xs text-text/90 sm:inline-flex sm:w-auto sm:text-sm">
              <span className="shrink-0 text-accent">$</span>
              <span className="min-w-0 whitespace-pre-wrap [overflow-wrap:anywhere]">curl -fsSL redstart-lang.com/install.sh | sh</span>
            </div>
          </Reveal>
        </div>
      </section>

      {/* ---- Proof ---- */}
      <section className="border-b border-line bg-elev">
        <div className="mx-auto max-w-6xl px-5 py-12 sm:py-16">
          <Reveal>
            <p className="eyebrow">Proven, not promised</p>
          </Reveal>
          <div className="mt-8 grid gap-10 sm:grid-cols-3 sm:gap-8">
            {PROOF.map((p, i) => (
              <Reveal key={p.unit} i={i} className="min-w-0">
                <div className="display text-5xl tabular-nums sm:text-6xl">
                  {p.n}
                  <span className="ml-2 font-mono text-sm uppercase tracking-[0.12em] text-accent">
                    {p.unit}
                  </span>
                </div>
                <p className="mt-3 max-w-[34ch] text-muted">{p.d}</p>
              </Reveal>
            ))}
          </div>
        </div>
      </section>

      {/* ---- Would you rather maintain this, or this? ---- */}
      <section className="border-b border-line">
        <div className="mx-auto max-w-6xl px-5 py-16 sm:py-24">
          <Reveal>
            <p className="eyebrow">The pitch</p>
            <h2 className="display mt-3 max-w-2xl text-4xl sm:text-5xl">
              Would you rather write and maintain this
            </h2>
          </Reveal>

          <div className="mt-12 grid items-start gap-6 lg:grid-cols-2">
            <Reveal i={1} className="flex min-w-0 flex-col gap-3">
              <div className="flex items-center justify-between gap-3">
                <span className="inline-flex items-center gap-2 text-sm font-medium">
                  <Bird className="h-4 w-4 text-accent" /> Redstart
                </span>
                <span className="font-mono text-xs text-muted">
                  1 file · drift impossible
                </span>
              </div>
              <CodeBlock code={REDSTART_ERC20} lang="red" filename="token.red" />
            </Reveal>

            <Reveal i={2} className="flex min-w-0 flex-col gap-3">
              <div className="flex items-center justify-between gap-3">
                <span className="text-sm font-medium text-muted">
                  …or these three, kept in sync by hand?
                </span>
                <span className="font-mono text-xs text-muted">
                  3 files · names must agree
                </span>
              </div>
              <FileTabs
                muted
                className="min-h-[360px]"
                files={[
                  { name: "mappings.ts", lang: "ts", code: AS_MAPPINGS },
                  { name: "schema.graphql", lang: "graphql", code: AS_SCHEMA },
                  { name: "subgraph.yaml", lang: "yaml", code: AS_MANIFEST },
                ]}
              />
            </Reveal>
          </div>
          <Reveal i={3}>
            <p className="mt-8 max-w-2xl text-muted">
              Both produce the same store. Only one of them can&apos;t drift,
              can&apos;t forget a{" "}
              <code className="font-mono text-text">.save()</code>, and
              can&apos;t let a renamed event compile. The AssemblyScript is the
              genuine hand-written reference from our conformance suite, not a
              strawman.
            </p>
          </Reveal>
        </div>
      </section>

      {/* ---- Footguns ---- */}
      <section className="border-b border-line">
        <div className="mx-auto max-w-6xl px-5 py-16 sm:py-24">
          <Reveal>
            <p className="eyebrow">Unrepresentable by construction</p>
            <h2 className="display mt-3 max-w-2xl text-4xl sm:text-5xl">
              A whole class of bugs you can&apos;t write.
            </h2>
          </Reveal>
          <div className="mt-12 grid gap-px overflow-hidden rounded-2xl border border-line bg-line sm:grid-cols-2 lg:grid-cols-3">
            {FOOTGUNS.map((f, i) => (
              <Reveal
                key={f.t}
                i={i % 3}
                as="article"
                className="bg-elev p-7 sm:last:col-span-2 lg:last:col-span-1"
              >
                <div className="flex items-center gap-2 text-sm font-medium">
                  <span className="font-mono text-accent">✕</span>
                  {f.t}
                </div>
                <p className="mt-2.5 text-sm leading-relaxed text-muted">
                  {f.d}
                </p>
              </Reveal>
            ))}
          </div>
        </div>
      </section>

      {/* ---- Eject path ---- */}
      <section className="border-b border-line">
        <div className="mx-auto grid max-w-6xl gap-12 px-5 py-16 sm:py-24 lg:grid-cols-[1.1fr_1fr] lg:items-center">
          <Reveal>
            <p className="eyebrow">No lock-in</p>
            <h2 className="display mt-3 text-4xl sm:text-5xl">
              The eject path is the whole bet.
            </h2>
            <p className="mt-5 max-w-lg leading-relaxed text-muted">
              <code className="font-mono text-text">redstart build</code> emits
              the exact files a hand-written subgraph has: readable, idiomatic
              AssemblyScript and GraphQL that{" "}
              <code className="font-mono text-text">graph build</code> compiles
              unmodified. Walk away whenever you like; you keep the generated
              code, and it keeps working.
            </p>
          </Reveal>
          <Reveal i={1}>
            <div className="card p-7 font-mono text-sm">
              <div className="flex items-center gap-2 text-text">
                <Bird className="h-4 w-4 text-accent" /> token.red
              </div>
              <div className="my-4 ml-1.5 border-l border-line pl-5 text-muted">
                <div className="relative">
                  <span className="absolute -left-[1.42rem] text-accent">↓</span>
                  redstart build
                </div>
              </div>
              <div className="grid gap-2 text-text/90">
                <div className="rounded-md border border-line bg-inset px-3 py-2">
                  schema.graphql
                </div>
                <div className="rounded-md border border-line bg-inset px-3 py-2">
                  subgraph.yaml
                </div>
                <div className="rounded-md border border-line bg-inset px-3 py-2">
                  src/mappings.ts
                </div>
              </div>
              <div className="my-4 ml-1.5 border-l border-line pl-5 text-muted">
                <div className="relative">
                  <span className="absolute -left-[1.42rem] text-accent">↓</span>
                  graph codegen · graph build
                </div>
              </div>
              <div className="rounded-md bg-accent px-3 py-2 text-center font-medium text-on-accent">
                compiles unmodified → WASM
              </div>
            </div>
          </Reveal>
        </div>
      </section>

      {/* ---- Scope ---- */}
      <section className="border-b border-line">
        <div className="mx-auto max-w-6xl px-5 py-16 sm:py-24">
          <Reveal>
            <p className="eyebrow">Scope</p>
            <h2 className="display mt-3 max-w-2xl text-4xl sm:text-5xl">
              What it does, and what it doesn&apos;t.
            </h2>
          </Reveal>
          <div className="mt-12 grid gap-px overflow-hidden rounded-[10px] border border-line bg-line md:grid-cols-2">
            {[
              { label: "It does", tone: "text-ok", items: DOES },
              { label: "It doesn\u2019t", tone: "text-accent", items: DOESNT },
            ].map((col, i) => (
              <Reveal key={col.label} i={i} className="min-w-0 bg-elev p-7">
                <p className={`font-mono text-xs uppercase tracking-[0.12em] ${col.tone}`}>
                  {col.label}
                </p>
                <ul className="mt-5 space-y-3">
                  {col.items.map((item) => (
                    <li key={item} className="flex gap-3 leading-relaxed text-muted">
                      <span aria-hidden className={`font-mono ${col.tone}`}>
                        {i === 0 ? "+" : "-"}
                      </span>
                      {item}
                    </li>
                  ))}
                </ul>
              </Reveal>
            ))}
          </div>
          <Reveal>
            <p className="mt-8 max-w-2xl text-muted">
              The{" "}
              <a href={`${DOCS}porting.html`} className="text-accent hover:underline">
                porting guide
              </a>{" "}
              covers each limit, and the pattern that replaces receipt inspection.
            </p>
          </Reveal>
        </div>
      </section>

      {/* ---- Final CTA ---- */}
      <section className="relative overflow-hidden">
        <div className="relative mx-auto max-w-6xl px-5 py-20 sm:py-28 text-center">
          <Reveal>
            <Bird className="mx-auto h-12 w-12 text-accent" />
            <h2 className="display mx-auto mt-6 max-w-2xl text-4xl sm:text-6xl">
              See the drift <span className="text-accent">disappear.</span>
            </h2>
            <p className="mx-auto mt-5 max-w-md text-muted">
              The playground runs the real compiler in your browser. Type
              Redstart, watch the three files generate live.
            </p>
            <div className="mt-8 flex flex-wrap justify-center gap-3">
              <Link href="/playground" className="btn">
                Open the playground
                <span aria-hidden>→</span>
              </Link>
              <a href={REPO} className="btn btn-ghost">
                Read the source
              </a>
            </div>
          </Reveal>
        </div>
      </section>
    </>
  );
}
