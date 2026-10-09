import Link from "next/link";
import { Bird } from "./logo";

const REPO = "https://github.com/nuthatch-org/redstart";
const DOCS = "https://nuthatch-org.github.io/redstart/";
// Short build id so you can eyeball which deploy a browser is actually running.
const BUILD = (process.env.NEXT_PUBLIC_BUILD_ID || "dev").slice(0, 7);

export function Footer() {
  return (
    <footer className="relative border-t border-line">
      <div className="mx-auto grid max-w-6xl gap-8 px-5 py-14 sm:grid-cols-[1.5fr_1fr_1fr]">
        <div>
          <span className="inline-flex items-center gap-2 text-[1.05rem]">
            <Bird className="h-[1.15em] w-[1.15em] text-red" />
            <span className="font-medium tracking-tight">Redstart</span>
          </span>
          <p className="mt-3 max-w-xs text-sm leading-relaxed text-muted">
            One typed language for The Graph subgraphs. An open-source public
            good, in the lineage of Matchstick.
          </p>
        </div>
        <div className="flex flex-col gap-2 text-sm text-muted">
          <span className="eyebrow mb-1">Product</span>
          <Link href="/generator" className="transition-colors hover:text-text">
            Generator
          </Link>
          <Link href="/playground" className="transition-colors hover:text-text">
            Playground
          </Link>
          <a href={DOCS} target="_blank" rel="noopener noreferrer" className="transition-colors hover:text-text">
            Documentation
          </a>
        </div>
        <div className="flex flex-col gap-2 text-sm text-muted">
          <span className="eyebrow mb-1">Source</span>
          <a href={REPO} target="_blank" rel="noopener noreferrer" className="transition-colors hover:text-text">
            GitHub
          </a>
          <a href={`${REPO}/releases`} target="_blank" rel="noopener noreferrer" className="transition-colors hover:text-text">
            Releases
          </a>
          <a href={`${REPO}/tree/main/rfcs`} target="_blank" rel="noopener noreferrer" className="transition-colors hover:text-text">
            RFCs
          </a>
        </div>
      </div>
      <div className="border-t border-line">
        <div className="mx-auto flex max-w-6xl items-center justify-between px-5 py-4 text-xs text-faint">
          <span>MIT licensed · The Lodestar Team</span>
          <span className="font-mono">
            redstart-lang.com <span className="text-faint/60">· build {BUILD}</span>
          </span>
        </div>
      </div>
    </footer>
  );
}
