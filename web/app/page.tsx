import Link from "next/link";
import { Reveal } from "@/components/reveal";
import { CodeBlock } from "@/components/code";
import { FileTabs } from "@/components/file-tabs";
import { Bird } from "@/components/logo";
import { Constellation } from "@/components/constellation";
import {
  REDSTART_ERC20,
  AS_MAPPINGS,
  AS_SCHEMA,
  AS_MANIFEST,
} from "@/lib/samples";

const REPO = "https://github.com/nuthatch-org/redstart";

const FOOTGUNS = [
  {
    t: "Forgotten .save()",
    d: "Entities are dirty-tracked and flushed at handler end. The most common subgraph bug isn't expressible.",
  },
  {
    t: "Arithmetic on null",
    d: "There is no null — only Option<T>. Maths on a maybe-absent value is a compile error, not a silent miscompile.",
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
    d: "Date.now() and Math.random() are a compile error — they diverge Proof-of-Indexing across indexers and get them slashed.",
  },
  {
    t: "eth_call in a loop",
    d: "A contract call inside a loop — the documented 'stuck at 3%' sync killer — is flagged before you deploy it.",
  },
];

export default function Home() {
  return (
    <>
      {/* ---- Hero ---- */}
      <section className="relative overflow-hidden border-b border-line">
        <div className="relative mx-auto max-w-6xl px-5 pb-16 pt-16 sm:pb-24 sm:pt-28">
          <Constellation className="pointer-events-none absolute right-0 top-6 hidden h-[420px] w-[420px] opacity-90 lg:block" />
          <Reveal>
            <span className="tag">
              <Bird className="h-3.5 w-3.5" />
              the sexiest language for authoring The Graph subgraphs
            </span>
          </Reveal>
          <Reveal i={1}>
            <h1 className="display mt-6 max-w-3xl text-4xl sm:text-6xl md:text-7xl">
              Write the subgraph once.
              <br />
              <span className="grad">Properly.</span>
            </h1>
          </Reveal>
          <Reveal i={2}>
            <p className="mt-6 max-w-xl text-lg leading-relaxed text-muted">
              The most performant and secure language for authoring The Graph
              subgraphs. One typed source for schema, manifest, and mappings —
              compiled to AssemblyScript that&apos;s faster and safer than any
              human would hand-write. <span className="text-text">If it compiles,
              it works.</span>
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
            <div className="mt-6 flex w-full max-w-full items-center gap-3 overflow-x-auto rounded-lg border border-line bg-surface px-4 py-2.5 font-mono text-xs text-text/90 sm:inline-flex sm:w-auto sm:text-sm">
              <span className="shrink-0 text-red-bright">$</span>
              <span className="whitespace-nowrap">curl -fsSL redstart-lang.com/install.sh | sh</span>
            </div>
          </Reveal>
        </div>
      </section>

      {/* ---- Would you rather maintain this — or this? ---- */}
      <section className="border-b border-line">
        <div className="mx-auto max-w-6xl px-5 py-16 sm:py-24">
          <Reveal>
            <p className="eyebrow">The pitch</p>
            <h2 className="display mt-3 max-w-2xl text-4xl sm:text-5xl">
              Would you rather write &amp; maintain this —
            </h2>
          </Reveal>

          <div className="mt-12 grid items-start gap-6 lg:grid-cols-2">
            <Reveal i={1} className="flex flex-col gap-3">
              <div className="flex items-center justify-between">
                <span className="inline-flex items-center gap-2 text-sm font-medium">
                  <Bird className="h-4 w-4 text-red" /> Redstart
                </span>
                <span className="font-mono text-xs text-faint">
                  1 file · drift impossible
                </span>
              </div>
              <CodeBlock code={REDSTART_ERC20} lang="red" filename="token.red" />
            </Reveal>

            <Reveal i={2} className="flex flex-col gap-3">
              <div className="flex items-center justify-between">
                <span className="text-sm font-medium text-muted">
                  …or these three, kept in sync by hand?
                </span>
                <span className="font-mono text-xs text-faint">
                  3 files · names must agree
                </span>
              </div>
              <FileTabs
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
              can&apos;t let a renamed event compile. The AssemblyScript on the
              right is the genuine hand-written reference from our conformance
              suite — not a strawman.
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
                className="group bg-bg-2 p-7 transition-colors hover:bg-surface"
              >
                <div className="flex items-center gap-2 text-sm font-medium">
                  <span className="font-mono text-red-bright">✕</span>
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
              the exact files a hand-written subgraph has — readable, idiomatic
              AssemblyScript and GraphQL that{" "}
              <code className="font-mono text-text">graph build</code> compiles
              unmodified. Walk away whenever you like; you keep the generated
              code, and it keeps working.
            </p>
            <p className="mt-4 max-w-lg leading-relaxed text-muted">
              And it&apos;s <span className="text-text">proven</span>, not promised:
              deployed to a live graph-node against an independent hand-written
              reference, our lowered AssemblyScript store-diffed{" "}
              <span className="text-text">0 differences</span> across every entity
              and field — byte-identical indexing.
            </p>
          </Reveal>
          <Reveal i={1}>
            <div className="card p-7 font-mono text-sm">
              <div className="flex items-center gap-2 text-text">
                <Bird className="h-4 w-4 text-red" /> token.red
              </div>
              <div className="my-4 ml-1.5 border-l border-line pl-5 text-faint">
                <div className="relative">
                  <span className="absolute -left-[1.42rem] text-red-bright">↓</span>
                  redstart build
                </div>
              </div>
              <div className="grid gap-2 text-text/90">
                <div className="rounded-md border border-line bg-surface px-3 py-2">
                  schema.graphql
                </div>
                <div className="rounded-md border border-line bg-surface px-3 py-2">
                  subgraph.yaml
                </div>
                <div className="rounded-md border border-line bg-surface px-3 py-2">
                  src/mappings.ts
                </div>
              </div>
              <div className="my-4 ml-1.5 border-l border-line pl-5 text-faint">
                <div className="relative">
                  <span className="absolute -left-[1.42rem] text-red-bright">↓</span>
                  graph codegen · graph build
                </div>
              </div>
              <div
                className="rounded-md px-3 py-2 text-center font-medium text-white"
                style={{ background: "linear-gradient(120deg, #ff3355, #ff7a45)" }}
              >
                compiles unmodified → WASM
              </div>
            </div>
          </Reveal>
        </div>
      </section>

      {/* ---- Final CTA ---- */}
      <section className="relative overflow-hidden">
        <div
          className="pointer-events-none absolute left-1/2 top-1/2 h-[60vh] w-[60vh] -translate-x-1/2 -translate-y-1/2 rounded-full"
          style={{ background: "radial-gradient(closest-side, rgba(255,51,85,0.18), transparent)" }}
        />
        <div className="relative mx-auto max-w-6xl px-5 py-20 sm:py-28 text-center">
          <Reveal>
            <Bird glow className="mx-auto h-12 w-12 text-red" />
            <h2 className="display mx-auto mt-6 max-w-2xl text-4xl sm:text-6xl">
              See the drift <span className="grad">disappear.</span>
            </h2>
            <p className="mx-auto mt-5 max-w-md text-muted">
              The playground runs the real compiler in your browser. Type
              Redstart, watch the three files generate live.
            </p>
            <div className="mt-8 flex justify-center gap-3">
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
