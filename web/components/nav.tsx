import Link from "next/link";
import { Wordmark } from "./logo";

const REPO = "https://github.com/nuthatch-org/redstart";
const DOCS = "https://nuthatch-org.github.io/redstart/";

export function Nav() {
  return (
    <header className="sticky top-0 z-50 border-b border-line bg-bg/70 backdrop-blur-xl">
      <div className="mx-auto flex h-14 max-w-6xl items-center justify-between px-4 sm:px-5">
        <Link href="/" className="text-[1.05rem] text-text">
          <Wordmark />
        </Link>
        <nav className="-mr-2 flex items-center text-sm text-muted sm:mr-0 sm:gap-1">
          <Link
            href="/generator"
            className="rounded-md px-2.5 py-1.5 font-medium text-accent transition-colors hover:bg-inset sm:px-3"
          >
            Generator
          </Link>
          <Link
            href="/playground"
            className="rounded-md px-2.5 py-1.5 transition-colors hover:bg-inset hover:text-text sm:px-3"
          >
            Playground
          </Link>
          <a
            href={DOCS}
            target="_blank"
            rel="noopener noreferrer"
            className="rounded-md px-2.5 py-1.5 transition-colors hover:bg-inset hover:text-text sm:px-3"
          >
            Docs
          </a>
          <a
            href={REPO}
            target="_blank"
            rel="noopener noreferrer"
            className="rounded-md px-2.5 py-1.5 transition-colors hover:bg-inset hover:text-text sm:px-3"
          >
            GitHub
          </a>
        </nav>
      </div>
    </header>
  );
}
